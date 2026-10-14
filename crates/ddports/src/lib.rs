//! The readable ports (crates/mtmath, mtcore, mtframe, mtdraw) and mods, registered
//! by address over their lifted bodies (L60, docs/NATIVE_PIPELINE.md,
//! "Overrides").
//!
//! The shards call each other and the platform models through overridable
//! dispatch (`x86::call_lifted`, `x86::call_local`); an override registered
//! here before the program runs takes the lifted body's place for every
//! caller. Each port crate's `abi::PORTS` lists its adapters; its
//! differential test replays them against the lifted vectors.
use ddnative_base::x86::{self, LiftFn};
// The port crates, for runtime-side installs that need the lifted bodies.
pub use mtcore;

/// Every verified readable port: (address, adapter).
pub fn ports() -> Vec<(u32, LiftFn)> {
    let mut ports: Vec<(u32, LiftFn)> = mtmath::abi::PORTS.iter().chain(mtcore::abi::PORTS).chain(mtcore::hot::PORTS).chain(mtframe::abi::PORTS).chain(mtdraw::abi::PORTS)
        .map(|&(a, f, _)| (a, f))
        .collect();
    // L158: exact inner dependency hash. Keep opt-in until the combined
    // native gate and head's matched per-job capture have accepted it.
    if std::env::var("DDRUNTIME_JOB_HASH").as_deref() == Ok("1") {
        ports.push((0x00e0e5e0, mtcore::job_hash::dependency_hash));
    }
    if std::env::var("DDRUNTIME_JOB_LOOKUP").as_deref() == Ok("1") {
        ports.push((0x01008200, mtcore::job_lookup::single_key));
        ports.push((0x01008260, mtcore::job_lookup::pair_key));
    }
    if std::env::var("DDRUNTIME_JOB_SHADOW_SAVE").as_deref() == Ok("1") {
        ports.push((0x00fed480, mtcore::job_shadow::save_state));
    }
    // L169: preserve the original serial/queued decision and callback order.
    if std::env::var("DDRUNTIME_SHADOW_TRAVERSAL").as_deref() == Ok("1") {
        ports.push((0x00db63e0, mtcore::shadow_traversal::traverse));
    }
    // L172 P2b: the sPrimitive begin/end methods and their dispatch thunks.
    if std::env::var("DDRUNTIME_SHADOW_PRIMITIVE").as_deref() == Ok("1") {
        ports.push((0x00e61010, mtcore::shadow_primitive::sprimitive_vf11));
        ports.push((0x00e68390, mtcore::shadow_primitive::sprimitive_vf12));
        ports.push((0x0107ff00, mtcore::shadow_primitive::dispatch_begin));
        ports.push((0x0107ff30, mtcore::shadow_primitive::dispatch_end));
        ports.push((0x0111e180, mtcore::shadow_primitive::sprimitive_sort));
    }
    if std::env::var("DDRUNTIME_SHADOW_PASS").as_deref() == Ok("1") {
        ports.push((0x00efd240, mtcore::shadow_pass::setup));
    }
    // L176: the nDraw material's parameter binding, colour and update
    // (semantic ports, crates/mtdraw/tests/material.rs). 0x01066590,
    // 0x010655A0 and 0x00FEDD60 are ported in L176 too but registered from
    // L170, L174 and L171.
    if std::env::var("DDRUNTIME_MATERIAL").as_deref() == Ok("1") {
        ports.push((0x0106_6b10, mtdraw::material::bind_parameter));
        ports.push((0x0106_7790, mtdraw::material::material_colour));
        ports.push((0x0106_2f20, mtdraw::material::update_material));
    }
    // L176: the draw context's render-target setter (semantic port,
    // crates/mtdraw/tests/material.rs).
    if std::env::var("DDRUNTIME_SET_TARGET").as_deref() == Ok("1") {
        ports.push((0x00fe_fb90, mtdraw::target::set_render_target));
    }
    // L176: the draw context's frustum tests (semantic ports,
    // crates/mtdraw/tests/material.rs).
    if std::env::var("DDRUNTIME_CULL").as_deref() == Ok("1") {
        ports.push((0x00ff_0780, mtdraw::cull::cull_sphere));
        ports.push((0x00ff_0ad0, mtdraw::cull::cull_oriented));
        ports.push((0x00ff_0d20, mtdraw::cull::cull_box));
        ports.push((0x00ff_0e60, mtdraw::cull::cull_box_all));
    }
    // L176: sSceneExt::vf16, the draw's light setup (semantic port,
    // crates/mtdraw/tests/material.rs).
    if std::env::var("DDRUNTIME_LIGHTS").as_deref() == Ok("1") {
        ports.push((0x0049_b1a0, mtdraw::lights::scene_lights));
    }
    // L171: semantic (ABI-level) rewrites replace the instruction-level
    // ports at the same addresses.
    if std::env::var("DDRUNTIME_SEMANTIC").as_deref() == Ok("1") {
        for (a, f) in [
            (0x00e0ec50u32, mtcore::semantic::expand_dependencies as LiftFn),
            (0x00e10fc0, mtcore::semantic::intern_dependency_keys),
            (0x01054700, mtcore::semantic::update_joints),
            (0x00fedd60, mtcore::semantic::sort_pairs),
            (0x00ea23f0, mtcore::semantic_draw_c::primitive_material),
            (0x00fed190, mtcore::semantic_draw_c::begin_material),
            (0x00e0f3c0, mtcore::semantic_draw_c::package_key),
            (0x01008200, mtcore::semantic_draw_c::single_key),
            (0x00e0e5e0, mtcore::semantic_draw_b::dependency_hash),
            (0x010655a0, mtcore::semantic_draw_b::apply_material),
            (0x01066c60, mtcore::semantic_draw_b::select_material_pass),
            (0x00ff1920, mtcore::semantic_draw_a::shadow_draws),
            (0x00ff14d0, mtcore::semantic_draw_a::replay_draw_passes),
            (0x00fefb90, mtcore::semantic_draw_a::bind_render_target),
            (0x00ea5320, mtcore::semantic_draw::draw_mesh),
            (0x00fee320, mtcore::semantic_draw_c::commit_primitive_state),
            (0x00ffc350, mtcore::semantic_draw_c::draw_model_mesh),
            (0x01145bf0, mtcore::semantic_draw_c::apply_standard_material),
            (0x00ffc9f0, mtcore::semantic_draw_b::prepare_model_palette),
            (0x01140180, mtcore::semantic_draw_b::draw_culled_instances),
            (0x010a35d0, mtcore::semantic_jobs::query_bounds_tree),
            (0x00c24640, mtcore::semantic_instancing::draw_instanced_mesh),
            (0x010658b0, mtcore::semantic_c2::apply_pass_state),
            (0x00cd8760, mtcore::semantic::find_identifier),
            (0x00dd42c0, mtcore::semantic::dispatch_event),
            (0x005b31a0, mtcore::semantic::find_sorted),
        ] {
            if !semantic_selected(a) {
                continue;
            }
            ports.retain(|p| p.0 != a);
            ports.push((a, f));
        }
    }
    // L184: the Sonnet subagents' semantic ports (recorded-case proofs in
    // crates/mtcore/tests/semantic_s*.rs). Opt-in as one batch; the
    // DDRUNTIME_SEMANTIC_ONLY/SKIP lists select single addresses.
    if std::env::var("DDRUNTIME_SEMANTIC_L184").as_deref() == Ok("1") {
        // s3: classify_overlap calls the native frustum tests directly
        let _ = mtcore::semantic_s3::CULL_PORTS.set((mtdraw::cull::cull_sphere, mtdraw::cull::cull_oriented));
        for (a, f) in [
            (0x005d_1af0u32, mtcore::semantic_s1::update_effect_timers as LiftFn),
            (0x0084_c0a0, mtcore::semantic_s1::update_character_frame),
            (0x0076_f2d0, mtcore::semantic_s1::refresh_actor_slots),
            (0x00c8_8380, mtcore::semantic_s1::sync_standard_material),
            (0x00c2_5ea0, mtcore::semantic_s1::update_instancing),
            (0x00e6_d070, mtcore::semantic_s1::count_active_children),
            (0x00fa_c6f0, mtcore::semantic_s2::probe_segment),
            (0x0079_e200, mtcore::semantic_s2::sweep_step),
            (0x0116_6c00, mtcore::semantic_s2::cast_shape_b),
            (0x0116_6b10, mtcore::semantic_s2::cast_shape_a),
            (0x00de_a870, mtcore::semantic_s2::world_query),
            (0x0115_f650, mtcore::semantic_s2::segment_cast),
            (0x00c5_2a30, mtcore::semantic_s3::finish_draw),
            (0x00ff_a620, mtcore::semantic_s3::update_model_bounds),
            (0x0076_b470, mtcore::semantic_s3::update_scene_objects),
            (0x007a_5fa0, mtcore::semantic_s3::sync_runtime_object),
            (0x0079_4550, mtcore::semantic_s3::apply_effect_toggle),
            (0x00e7_0710, mtcore::semantic_s4::actor_update),
            (0x00b6_6b30, mtcore::semantic_s4_player::player_post_update),
            (0x0105_4860, mtcore::semantic_s4_model::model_refresh),
            (0x0086_0ac0, mtcore::semantic_s5::constraint_edit_apply),
            (0x0087_2420, mtcore::semantic_s6::motion_filter_apply),
            (0x0082_9ec0, mtcore::semantic_s6_aim::aim_pose),
            (0x00f2_b5e0, mtcore::semantic_s7::joint_offset_update),
            (0x00e5_16f0, mtcore::semantic_s8::init_chain),
            (0x0113_0a00, mtcore::semantic_s8::look_at_frame),
            (0x00fe_d5d0, mtcore::semantic_s9::pop_render_state),
            (0x00ea_5b50, mtcore::semantic_s9::model_upload_state),
            (0x00ff_1310, mtcore::semantic_s9::set_fade),
            (0x00fe_b1f0, mtcore::semantic_s9::scene_collect_visible),
            (0x0077_81a0, mtcore::semantic_s1::tick_gauges),
            (0x00de_50b0, mtcore::semantic_s1::load_triangle_vertices),
            (0x00ea_42e0, mtcore::semantic_s1::cull_bounds),
            (0x00f2_3260, mtcore::semantic_s1::draw_attachment),
            (0x0116_69b0, mtcore::semantic_s2::walk_single_forward),
            (0x0116_6a60, mtcore::semantic_s2::walk_single_backward),
            (0x0116_67b0, mtcore::semantic_s2::walk_low_forward),
            (0x0116_68a0, mtcore::semantic_s2::walk_low_backward),
            (0x0116_5e00, mtcore::semantic_s2::walk_high_forward),
            (0x0116_5ef0, mtcore::semantic_s2::walk_high_backward),
            (0x0117_87c0, mtcore::semantic_s2::node_dispatch),
            (0x00d5_3730, mtcore::semantic_s3::closest_point_on_box),
            (0x00d5_3f30, mtcore::semantic_s3::distance_sq_to_box),
            (0x00d1_3ab0, mtcore::semantic_s3::sphere_touches_box),
            (0x00ea_1eb0, mtcore::semantic_s3::copy_position),
            (0x00ea_1f40, mtcore::semantic_s3::world_position),
            (0x00ea_1ee0, mtcore::semantic_s3::classify_overlap),
            (0x0111_d610, mtcore::semantic_s3::push_state),
            (0x00e6_1010, mtcore::semantic_s3::any_member_test),
            (0x0081_fd10, mtcore::semantic_s4_joint::joint_rebase),
            (0x0086_b2e0, mtcore::semantic_s5::scale_translate_in_parent_space),
            (0x0086_4660, mtcore::semantic_s5::aim_twist_provider),
            (0x0086_5990, mtcore::semantic_s5::euler_twist_provider),
            (0x00d1_b840, mtcore::semantic_s7_collision::segment_vs_box),
            (0x00f2_ffc0, mtcore::semantic_s7_falloff::integrate_falloff),
            (0x012f_05c0, mtcore::semantic_s7_ftol::ftol2_sse),
            (0x00d7_cf70, mtcore::semantic_s8_sweep::sweep_capsule_triangle),
            (0x00d6_3ff0, mtcore::semantic_s8_sweep::shape_support),
            (0x00d1_b540, mtcore::semantic_s8_sweep::simplex_add_point),
            (0x00d1_2e10, mtcore::semantic_s8_sweep::simplex_segment_closest),
            (0x00ff_2ee0, mtcore::semantic_s9::set_camera_matrices),
            (0x00ff_04c0, mtcore::semantic_s9::set_viewport_rect),
            (0x0049_e920, mtcore::semantic_s10::find_nearest_object),
            (0x00fe_66a0, mtcore::semantic_s10::gather_ambient_probe),
            (0x00ea_3130, mtcore::semantic_p2c::base_model_draw_unit),
            (0x00ea_1fd0, mtcore::semantic_p2c::select_shadow_state_abi),
            (0x00fe_db80, mtcore::semantic_s1::save_recording_state),
            (0x00fe_dc00, mtcore::semantic_s1::restore_recording_state),
            (0x00cf_eec0, mtcore::semantic_s7_stream::stream_skip),
            (0x00cf_ec00, mtcore::semantic_s7_stream::stream_read),
            (0x00cf_e9d0, mtcore::semantic_s7_stream::stream_refill),
            (0x00ea_5320, mtcore::semantic_draw::draw_mesh_folded),
            (0x00f1_1610, mtcore::semantic_s9::draw_with_resources),
            (0x00c8_5a00, mtcore::semantic_s10::apply_param_block),
            (0x007f_fcc0, mtcore::semantic_s10::submit_pass_records),
            (0x00b8_a7f0, mtcore::semantic_s10::player_prepare_draw),
            (0x00fe_4780, mtcore::semantic_s10::bind_consumer_to_model),
            // L191: per-model draw prep (uObjModel::vf11)
            (0x0088_6880, mtcore::semantic_l191::obj_model_vf11),
            (0x0088_5dc0, mtcore::semantic_l191::obj_model_vf08),
            (0x0084_c720, mtcore::semantic_l191::character_vf09),
            (0x0088_6530, mtcore::semantic_l191::obj_model_vf09),
            (0x0116_5b40, mtcore::semantic_l191::probe_search_step),
            (0x0116_b670, mtcore::semantic_l191::owner_visit),
        ] {
            if !semantic_selected(a) {
                continue;
            }
            ports.retain(|p| p.0 != a);
            ports.push((a, f));
        }
    }
    // L171: semantic ports not yet checked on recorded vectors
    if std::env::var("DDRUNTIME_SEMANTIC_DRAW").as_deref() == Ok("1") {
        for (a, f) in [(0x00ff1090u32, mtcore::semantic::set_pass as LiftFn)] {
            ports.retain(|p| p.0 != a);
            ports.push((a, f));
        }
    }
    ports
}

/// Whether the semantic rewrite at `address` is in use: every rewrite
/// unless `DDRUNTIME_SEMANTIC_ONLY` lists the addresses to keep, or
/// `DDRUNTIME_SEMANTIC_SKIP` the addresses to leave out (comma-separated
/// hex, `0x` optional). One build can then bisect the rewrites in game.
pub fn semantic_selected(address: u32) -> bool {
    let listed = |name: &str| {
        std::env::var(name).ok().map(|list| {
            list.split(',').any(|item| {
                let item = item.trim();
                let hex = item.strip_prefix("0x").unwrap_or(item);
                u32::from_str_radix(hex, 16) == Ok(address)
            })
        })
    };
    listed("DDRUNTIME_SEMANTIC_ONLY").unwrap_or(true) && !listed("DDRUNTIME_SEMANTIC_SKIP").unwrap_or(false)
}

/// The ports with `mods` over them: a mod at a ported address replaces the
/// port.
pub fn with_mods(mods: &[(u32, LiftFn)]) -> Vec<(u32, LiftFn)> {
    let mut v: Vec<(u32, LiftFn)> = ports().into_iter().filter(|(a, _)| !mods.iter().any(|m| m.0 == *a)).collect();
    v.extend_from_slice(mods);
    v
}

/// Register the ports and `mods`, once, before the lifted program runs.
pub fn install(mods: &[(u32, LiftFn)]) {
    x86::set_overrides(&with_mods(mods));
}

static MODS: std::sync::OnceLock<Vec<(u32, LiftFn)>> = std::sync::OnceLock::new();

/// A mod's overrides, set before the program starts (before [`ensure`]);
/// false if mods were already set.
pub fn set_mods(mods: Vec<(u32, LiftFn)>) -> bool {
    MODS.set(mods).is_ok()
}

/// Register the ports and the mods set so far, once per process (the
/// runtime calls this before it lists the lifted program).
pub fn ensure() {
    ensure_with(|_| {});
}

/// Compose diagnostic wrappers with ports/mods before dispatch freezes.
/// Called once; a wrapper must retain and call the body it replaces.
pub fn ensure_with(wrap: impl FnOnce(&mut Vec<(u32, LiftFn)>)) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let mut v = with_mods(MODS.get().map(|v| &v[..]).unwrap_or(&[]));
        wrap(&mut v);
        x86::set_overrides(&v);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use ddnative_base::mem::Mem;
    use ddnative_base::x86::Cpu;

    fn a_mod(_: &mut dyn Mem, cpu: &mut Cpu) {
        cpu.eax = 7;
    }

    #[test]
    fn ports_are_unique_and_a_mod_replaces_a_port() {
        let p = ports();
        assert_eq!(p.iter().any(|&(a, f)| a == 0x00fed480 &&
            std::ptr::fn_addr_eq(f, mtcore::job_shadow::save_state as LiftFn)),
            std::env::var("DDRUNTIME_JOB_SHADOW_SAVE").as_deref() == Ok("1"));
        let lookup_enabled = std::env::var("DDRUNTIME_JOB_LOOKUP").as_deref() == Ok("1");
        let semantic_enabled = std::env::var("DDRUNTIME_SEMANTIC").as_deref() == Ok("1");
        for (address, body) in [
            (0x00e0e5e0, mtcore::semantic_draw_b::dependency_hash as LiftFn),
            (0x010655a0, mtcore::semantic_draw_b::apply_material),
            (0x01066c60, mtcore::semantic_draw_b::select_material_pass),
            (0x00ffc9f0, mtcore::semantic_draw_b::prepare_model_palette),
            (0x01140180, mtcore::semantic_draw_b::draw_culled_instances),
        ] {
            assert_eq!(p.iter().any(|&(a, f)| a == address && std::ptr::fn_addr_eq(f, body)), semantic_enabled);
        }
        for (address, body) in [
            (0x01008200, mtcore::job_lookup::single_key as LiftFn),
            (0x01008260, mtcore::job_lookup::pair_key as LiftFn),
        ] {
            assert_eq!(p.iter().any(|&(a, f)| a == address && std::ptr::fn_addr_eq(f, body)),
                lookup_enabled && !(address == 0x01008200 && semantic_enabled));
        }
        for (address, body) in [
            (0x00ea23f0, mtcore::semantic_draw_c::primitive_material as LiftFn),
            (0x00fed190, mtcore::semantic_draw_c::begin_material),
            (0x00e0f3c0, mtcore::semantic_draw_c::package_key),
            (0x01008200, mtcore::semantic_draw_c::single_key),
            (0x00fee320, mtcore::semantic_draw_c::commit_primitive_state),
            (0x00ffc350, mtcore::semantic_draw_c::draw_model_mesh),
            (0x01145bf0, mtcore::semantic_draw_c::apply_standard_material),
            (0x010a35d0, mtcore::semantic_jobs::query_bounds_tree),
            (0x00c24640, mtcore::semantic_instancing::draw_instanced_mesh),
            (0x010658b0, mtcore::semantic_c2::apply_pass_state),
        ] {
            assert_eq!(p.iter().any(|&(a, f)| a == address && std::ptr::fn_addr_eq(f, body)), semantic_enabled);
        }
        let mut addrs: Vec<u32> = p.iter().map(|e| e.0).collect();
        addrs.sort_unstable();
        addrs.dedup();
        assert_eq!(addrs.len(), p.len(), "an address ported twice");
        let first = p[0].0;
        let v = with_mods(&[(first, a_mod as LiftFn)]);
        assert_eq!(v.len(), p.len());
        assert!(v.iter().any(|&(a, f)| a == first && std::ptr::fn_addr_eq(f, a_mod as LiftFn)));
    }
}
