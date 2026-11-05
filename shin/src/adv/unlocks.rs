//! What the galleries unlock (the save's CG, BGM and movie sets, save spec 6.3): pictures and movies as their layers
//! load (`0x7100020470`, also for a loaded save's rebuilt scene), music as a new track starts (`0x71000337f0`), and
//! the UNLOCK command.

use shin_core::{
    format::{
        save::SaveVectors,
        scenario::{
            Scenario,
            info::{BgmId, MovieId, PictureId},
            instruction_elements::{TypedNumberArray, UntypedNumberArray, lower_number_array},
        },
    },
    vm::command::types::LayerType,
};

/// A picture or movie layer was loaded
pub fn layer_loaded(
    vectors: &mut SaveVectors,
    scenario: &Scenario,
    layer_type: LayerType,
    params: UntypedNumberArray,
) {
    let tables = scenario.info_tables();
    match layer_type {
        LayerType::Picture => {
            let (picture, ..): TypedNumberArray<PictureId> = lower_number_array(params);
            unlock_picture(vectors, scenario, picture);
        }
        LayerType::Movie => {
            let (movie, ..): TypedNumberArray<MovieId> = lower_number_array(params);
            vectors.unlocked_movies.add(movie.into_i32() as usize);
            let info = tables.movie_info(movie);
            unlock_picture(vectors, scenario, info.linked_picture_id);
            if let Some(bgm) = info.linked_bgm_id.repr() {
                bgm_started(vectors, scenario, bgm);
            }
        }
        _ => {}
    }
}

/// A picture and the CG linked to it
fn unlock_picture(vectors: &mut SaveVectors, scenario: &Scenario, picture: PictureId) {
    vectors.unlocked_cgs.add(picture.into_i32() as usize);
    if let Some(linked) = scenario.info_tables().picture_info(picture).linked_cg_id.repr() {
        vectors.unlocked_cgs.add(linked.into_i32() as usize);
    }
}

/// A new BGM track started: it and the track linked to it
pub fn bgm_started(vectors: &mut SaveVectors, scenario: &Scenario, bgm: BgmId) {
    vectors.unlocked_bgms.add(bgm.into_i32() as usize);
    let linked = scenario.info_tables().bgm_info(bgm).linked_bgm_id.into_i32();
    if linked >= 0 {
        vectors.unlocked_bgms.add(linked as usize);
    }
}

/// UNLOCK: type 0 pictures, 1 music, 2 movies
pub fn unlock_command(vectors: &mut SaveVectors, kind: u8, indices: impl IntoIterator<Item = i32>) {
    let set = match kind {
        0 => &mut vectors.unlocked_cgs,
        1 => &mut vectors.unlocked_bgms,
        2 => &mut vectors.unlocked_movies,
        _ => {
            tracing::warn!("UNLOCK: unknown type {kind}");
            return;
        }
    };
    for index in indices.into_iter().filter(|&index| index >= 0) {
        set.add(index as usize);
    }
}
