//! The script's commands (`shin::ADV::<NAME>`), one module each.
//!
//! A command from the VM is handled in two steps, as in the original: [`StartableCommand::apply_state`] updates the
//! scene model ([`VmState`]) the way the original's listener does, then [`StartableCommand::start`] acts on what is
//! drawn and heard and either finishes at once ([`CommandStartResult::Continue`]) or yields an
//! [`ExecutingCommand`] that is updated every frame until it returns its result (the original's execute slot, called
//! each frame until it returns true). While a save is loaded, [`replay_command`] stands in for both (slot 2): only the
//! model changes, and SGET, SELECT and QUIZ take their results from the route. The commands Umineko never uses finish
//! at once. See `research/docs/engine-internals.md` §10 and `research/docs/audit/commands.md`.

#![allow(clippy::upper_case_acronyms)]

mod prelude {
    pub use std::sync::Arc;

    pub use CommandStartResult::Yield;
    pub use shin_core::{
        format::scenario::Scenario,
        time::Ticks,
        vm::{
            command,
            command::{
                CommandResult,
                types::{VLayerId, VLayerIdRepr},
            },
        },
    };
    pub use tracing::warn;

    pub use crate::{
        adv::{AdvState, CommandStartResult, StartableCommand, UpdatableCommand, VmState},
        layer::DrawableLayer,
        update::UpdateContext,
    };
}

mod autosave;
mod bgmplay;
mod bgmstop;
mod bgmsync;
mod bgmvol;
mod chars;
mod debugout;
mod evbegin;
mod evend;
mod exit;
mod layerctrl;
mod layerinit;
mod layerload;
mod layerselect;
mod layerunload;
mod layerwait;
mod maskload;
mod moviewait;
mod msgclose;
mod msginit;
mod msgset;
mod msgsignal;
mod msgwait;
mod notifyset;
mod pageback;
mod planeclear;
mod planeselect;
mod quiz;
mod saveinfo;
mod select;
mod seonce;
mod sepan;
mod seplay;
mod sestop;
mod sestopall;
mod sevol;
mod sewait;
mod sget;
pub mod showchars;
mod sset;
mod syscall;
mod sysse;
mod tipsget;
mod trophy;
mod unlock;
mod voiceplay;
mod voicewait;
mod wait;
mod wipe;
mod wipewait;

use std::sync::Arc;

use derivative::Derivative;
use enum_dispatch::enum_dispatch;
use shin_core::{
    format::scenario::Scenario,
    vm::command::{CommandResult, RuntimeCommand},
};

use self::{
    bgmsync::BGMSYNC, layerload::LAYERLOAD, layerwait::LAYERWAIT, maskload::MASKLOAD,
    moviewait::MOVIEWAIT, msgclose::MSGCLOSE, msgset::MSGSET, msgwait::MSGWAIT, seonce::SEONCE,
    quiz::QUIZ, select::SELECT, showchars::SHOWCHARS, sewait::SEWAIT, voicewait::VOICEWAIT, wait::WAIT, wipe::WIPE, wipewait::WIPEWAIT,
};
use crate::{
    adv::{AdvState, VmState},
    update::UpdateContext,
};

#[enum_dispatch]
pub trait UpdatableCommand {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        is_fast_forwarding: bool,
    ) -> Option<CommandResult>;
}

// all commands that yield to the game loop should have:
// - a type implementing UpdatableCommand
// - a enum variant for that type here
#[enum_dispatch(UpdatableCommand)]
#[derive(Derivative)]
#[derivative(Debug)]
pub enum ExecutingCommand {
    #[derivative(Debug = "transparent")]
    WAIT,
    #[derivative(Debug = "transparent")]
    MSGSET,
    #[derivative(Debug = "transparent")]
    MSGWAIT,
    #[derivative(Debug = "transparent")]
    LAYERLOAD,
    #[derivative(Debug = "transparent")]
    LAYERWAIT,
    #[derivative(Debug = "transparent")]
    SEWAIT,
    #[derivative(Debug = "transparent")]
    MOVIEWAIT,
    #[derivative(Debug = "transparent")]
    WIPE,
    #[derivative(Debug = "transparent")]
    MASKLOAD,
    #[derivative(Debug = "transparent")]
    SEONCE,
    #[derivative(Debug = "transparent")]
    BGMSYNC,
    #[derivative(Debug = "transparent")]
    VOICEWAIT,
    #[derivative(Debug = "transparent")]
    WIPEWAIT,
    #[derivative(Debug = "transparent")]
    MSGCLOSE,
    #[derivative(Debug = "transparent")]
    SELECT,
    #[derivative(Debug = "transparent")]
    QUIZ,
    #[derivative(Debug = "transparent")]
    SHOWCHARS,
}

/// What a command does while a save is loaded (its replay slot, vtable slot 2)
pub enum ReplayStep {
    /// The command has been skipped (its effect on the scene model applied); the VM goes on with this result
    Continue(CommandResult),
    /// The replay stops before this command, which is then started live
    Stop(RuntimeCommand),
    /// A replayed choice reached the save point: the VM goes on live with this result
    Done(CommandResult),
}

/// Replays `command` towards the save point (`0x710000ee70` with each command's slot 2): nothing is shown or played;
/// SGET, SELECT and QUIZ take their results from the route, and the save-point commands stop once the counter
/// reaches the save point
pub fn replay_command(command: RuntimeCommand, state: &mut VmState) -> ReplayStep {
    use shin_core::format::save::RouteData;

    let save = &state.save;
    // the counter after a save-point command that would start now
    let stops_at = |counted: bool| save.counter + counted as u32 >= save.target;
    match command {
        RuntimeCommand::MSGSET(_) if stops_at(save.event.is_none()) => ReplayStep::Stop(command),
        RuntimeCommand::AUTOSAVE(_) | RuntimeCommand::EVBEGIN(_) | RuntimeCommand::EVEND(_)
            if stops_at(true) =>
        {
            ReplayStep::Stop(command)
        }
        RuntimeCommand::SELECT(_) | RuntimeCommand::QUIZ(_) if !save.message_current && stops_at(true) => {
            ReplayStep::Stop(command)
        }
        RuntimeCommand::EXIT(_) | RuntimeCommand::RESUME(_) => ReplayStep::Stop(command),
        RuntimeCommand::SELECT(v) => {
            v.apply_state(state);
            let pick = state.save.replay_answer(RouteData::SELECT_BITS);
            answered(state, v.token.finish(pick))
        }
        RuntimeCommand::QUIZ(v) => {
            v.apply_state(state);
            let answer = state.save.replay_answer(RouteData::QUIZ_BITS);
            answered(state, v.token.finish(answer))
        }
        RuntimeCommand::SGET(v) => ReplayStep::Continue(v.token.finish(state.save.replay_sget())),
        RuntimeCommand::SSET(v) => {
            state.save.sset(v.slot_number, v.value);
            ReplayStep::Continue(v.token.finish())
        }
        command => {
            macro_rules! apply_state {
                ($($cmd:ident),*) => {
                    match &command {
                        $(RuntimeCommand::$cmd(v) => {v.apply_state(state);},)*
                        _ => {}
                    }
                };
            }
            apply_state!(
                WAIT, MSGINIT, MSGSET, MSGWAIT, MSGSIGNAL, MSGCLOSE, WIPE, WIPEWAIT, BGMPLAY, BGMSTOP, BGMVOL,
                BGMSYNC, SEPLAY, SESTOP, SESTOPALL, SEVOL, SEPAN, SEWAIT, SEONCE, VOICEPLAY, VOICEWAIT, SYSSE,
                SAVEINFO, AUTOSAVE, EVBEGIN, EVEND, TROPHY, UNLOCK, LAYERINIT, LAYERLOAD, LAYERUNLOAD, LAYERCTRL,
                LAYERWAIT, LAYERSELECT, MOVIEWAIT, PAGEBACK, PLANESELECT, PLANECLEAR, MASKLOAD, CHARS, TIPSGET,
                SHOWCHARS, NOTIFYSET, DEBUGOUT, SYSCALL
            );
            ReplayStep::Continue(
                command
                    .execute_dummy()
                    .expect("BUG: only EXIT ends a dummy execution"),
            )
        }
    }
}

/// A replayed SELECT or QUIZ answer counts as a save point; reaching the save point there ends the replay
fn answered(state: &VmState, result: CommandResult) -> ReplayStep {
    if state.save.is_replaying() {
        ReplayStep::Continue(result)
    } else {
        ReplayStep::Done(result)
    }
}

pub fn apply_command_state_and_start(
    command: RuntimeCommand,
    context: &mut UpdateContext,
    scenario: &Arc<Scenario>,
    vm_state: &mut VmState,
    adv_state: &mut AdvState,
) -> CommandStartResult {
    macro_rules! impl_apply_state {
        ($($cmd:ident),*) => {
            match command {
                $(RuntimeCommand::$cmd(v) => {let info = v.apply_state(vm_state); v.start(context, scenario, vm_state, info, adv_state)},)*
                // the commands Umineko never uses (MSGSYNC, BGMWAIT, VOICESTOP, RESUMESET, RESUME, LAYERSWAP, TRANSSET,
                // TRANSWAIT, MASKUNLOAD) finish at once instead of stopping the game
                other => {
                    tracing::warn!("{:?} is not implemented; skipping it", other);
                    match other.execute_dummy() {
                        Some(result) => CommandStartResult::Continue(result),
                        None => CommandStartResult::Exit,
                    }
                }
            }
        };
        () => {};
    }

    impl_apply_state!(
        EXIT,
        SELECT,
        SGET,
        SSET,
        WAIT,
        MSGINIT,
        MSGSET,
        MSGWAIT,
        MSGSIGNAL,
        // MSGSYNC,
        MSGCLOSE,
        // SELECT,
        WIPE,
        WIPEWAIT,
        BGMPLAY,
        BGMSTOP,
        BGMVOL,
        // BGMWAIT,
        BGMSYNC,
        SEPLAY,
        SESTOP,
        SESTOPALL,
        SEVOL,
        SEPAN,
        SEWAIT,
        SEONCE,
        VOICEPLAY,
        // VOICESTOP,
        VOICEWAIT,
        SYSSE,
        SAVEINFO,
        AUTOSAVE,
        EVBEGIN,
        EVEND,
        // RESUMESET,
        // RESUME,
        // SYSCALL,
        TROPHY,
        UNLOCK,
        LAYERINIT,
        LAYERLOAD,
        LAYERUNLOAD,
        LAYERCTRL,
        LAYERWAIT,
        // LAYERSWAP,
        LAYERSELECT,
        MOVIEWAIT,
        // TRANSSET,
        // TRANSWAIT,
        PAGEBACK,
        PLANESELECT,
        PLANECLEAR,
        MASKLOAD,
        // MASKUNLOAD,
        CHARS,
        TIPSGET,
        QUIZ,
        SHOWCHARS,
        NOTIFYSET,
        DEBUGOUT,
        SYSCALL
    )
}

pub enum CommandStartResult {
    /// Continue VM execution
    Continue(CommandResult),
    /// Yield to the game loop, run the command to completion, execution continued with the result
    Yield(ExecutingCommand),
    /// EXIT: leave the adventure screen once its teardown is done
    Exit,
}

impl From<CommandResult> for CommandStartResult {
    fn from(result: CommandResult) -> Self {
        CommandStartResult::Continue(result)
    }
}

impl From<ExecutingCommand> for CommandStartResult {
    fn from(command: ExecutingCommand) -> Self {
        CommandStartResult::Yield(command)
    }
}

pub trait StartableCommand {
    /// Type of the information the commands needs smuggled from the `apply_state` to the `start` method
    ///
    /// When doing a command replay for scene loading, this information is ignored
    ///
    /// This usually corresponds to non-param data set in the command constructor
    // TODO: for scene loading, this info can actually be paseed to the `RunState` function (or whatever it would be)
    type StateInfo;

    fn apply_state(&self, state: &mut VmState) -> Self::StateInfo;
    fn start(
        self,
        context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        vm_state: &mut VmState,
        state_info: Self::StateInfo,
        adv_state: &mut AdvState,
    ) -> CommandStartResult;
}
