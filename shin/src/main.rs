//! The game: Umineko no Naku Koro ni Saku on PC (the Open Catbox Project), built on shin.
//!
//! [`app`] holds the scenes and the frame; [`adv`] is the story screen (the VM's commands, its menus and HUD);
//! [`layer`] and [`wiper`] draw it; [`audio`], [`asset`] and [`savedata`] play, load and save; the title flow and the
//! galleries have a module each; [`ui`] holds the shared gestures, button prompts and touch effect. The port-only parts
//! are [`port_settings`], [`bindings`], [`export`] and [`capture`]. The overall architecture is in
//! `research/docs/architecture.md`, the original's behaviour in `research/docs/engine-internals.md`.

// this is noisy & not well-supported by IDEs
#![allow(clippy::uninlined_format_args)]

extern crate self as shin;

use clap::Parser;
use tracing::error;

#[expect(unused)]
mod adv;
mod app;
mod asset;
mod audio;
mod bindings;
mod capture;
mod character_box;
mod cli;
mod export;
mod fps_counter;
mod layer;
mod random;
mod movie_box;
mod picture_box;
mod music_box;
mod port_settings;
mod render;
mod scenario_select;
mod savedata;
mod title;
mod title_demo;
mod ui;
mod update;
mod wiper;
// mod window;

fn main() {
    shin_window::init_tracing();
    let cli = cli::Cli::parse();

    // Create a background thread which checks for deadlocks every 10s
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(10));
            let deadlocks = parking_lot::deadlock::check_deadlock();
            if deadlocks.is_empty() {
                continue;
            }

            error!("{} deadlocks detected", deadlocks.len());
            for (i, threads) in deadlocks.iter().enumerate() {
                error!("Deadlock #{}", i);
                for t in threads {
                    error!("Thread Id {:#?}", t.thread_id());
                    error!("{:#?}", t.backtrace());
                }
            }
        }
    });

    shin_tasks::create_task_pools();

    shin_window::run_window::<app::App>(cli);
}
