//! Sequence-4 entrance ownership and native frame boundaries.
use super::{Dragon, DragonData, DragonEffect, DragonStep, HoverHeight, HoverKakkuTarget};
use ddai::think::{Actor, Inputs, Request, Thinker, Time};
use std::sync::Arc;

pub struct OpeningDragon {
    pub data: Arc<DragonData>,
    pub actor: Dragon,
    pub thinker: Thinker,
    pub target: [f32; 3],
    /// S29 explicitly stops the controller at the completed hover. S30
    /// enables the remaining landing/fire actions before removing this gate.
    pub entrance_only: bool,
    pub effects: Vec<DragonEffect>,
    pub world_requests: Vec<Request>,
    pub error: Option<String>,
    pub scale: [f32; 3],
    /// Explicit native-state override, when supplied by a host adapter.
    pub collision_conditions: Option<(bool, u32)>,
    /// uCharacterBase ctor 0x0084A100 initializes actor+0x2738 to zero.
    /// Scripted condition changes must update this canonical word.
    pub condition_flags: u32,
    /// cAICtrl ctor 0x0059DA00 initializes +0x0C's low byte to one.
    /// The active FSM update owns subsequent changes (0x0059F440).
    controller_flag_c: bool,
    /// Prior pass-C terrain result, read by the next phase90. None carries
    /// the constructor's zero result word, not an inferred ground height.
    pub scenery_result: Option<ddsim::sweep::Swept>,
    /// uEmDragonBase ctor 0x00A3E5E0 clears +0x679A. Runtime overrides
    /// must update this flag rather than silently suppressing GSF463.
    pub landing_notification_disabled: bool,
    cached_matrix: ddcombat::volumes::Mat4,
}

impl std::fmt::Debug for OpeningDragon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpeningDragon")
            .field("owner", &self.actor.fighter.owner)
            .field("position", &self.actor.pos)
            .field("states", &self.thinker.current_states())
            .field("error", &self.error)
            .finish()
    }
}

impl OpeningDragon {
    pub fn new(
        data: Arc<DragonData>,
        graph: Arc<mtfsm::Graph>,
        owner: u32,
        pos: [f32; 3],
        yaw: f32,
        scale: [f32; 3],
    ) -> Result<Self, String> {
        let actor = Dragon::new(&data, owner, pos, yaw);
        let cached_matrix = actor.model_matrix();
        Ok(Self {
            data,
            actor,
            thinker: Thinker::new("Dragon_OP", graph).map_err(|e| e.to_string())?,
            target: [0.0; 3],
            entrance_only: true,
            effects: vec![],
            world_requests: vec![],
            error: None,
            scale,
            collision_conditions: None,
            condition_flags: 0,
            controller_flag_c: true,
            scenery_result: None,
            landing_notification_disabled: false,
            cached_matrix,
        })
    }

    pub fn entrance_complete(&self) -> bool {
        self.thinker.current_states() == [3] && self.actor.done()
    }

    /// Enemy line, phases 90..93. The installed FSM reads only actual
    /// prior-frame action/motion completion. No synthetic feedback is used.
    pub fn pass_a(
        &mut self,
        step: f32,
        players: Vec<[f32; 3]>,
        sce_no: u32,
        fsm_flags: u32,
        area_stay: u32,
    ) -> Result<(), String> {
        self.effects.clear();
        self.world_requests.clear();
        self.cached_matrix = self.actor.model_matrix();
        // uObjModel::vf90 0x00885E30 reads the preceding terrain flags.
        // A3F820 disables the Dragon axis check; Press's previous phase91
        // enabled ground mode only when landed (0x00A6AF70).
        let ground_mode = self.actor.press.as_ref().is_some_and(|p| p.phase == super::attacks::PressPhase::Landed);
        self.actor.press_contact = Some((
            super::attacks::ground_contact(false, self.scenery_result.map_or(0, |s| s.met), ground_mode),
            !self.landing_notification_disabled,
        ));
        let inputs = Inputs {
            time: Time {
                delta: step,
                fps: ddsim::MOTION_FPS,
            },
            stage: Some(200),
            actor: Some(Actor {
                pos: self.actor.pos,
                world_pos: self.actor.pos,
                action_done: self.actor.done(),
            }),
            motion_frame: self.actor.frame,
            motion_ended: self.actor.motion_ended,
            players,
            sce_no,
            fsm_flags,
            area_stay,
            ..Default::default()
        };
        if !(self.entrance_only && self.entrance_complete()) {
            let requests = self
                .thinker
                .controlled_frame(&inputs)
                .map_err(|e| e.to_string())?;
            // 0x0059F440 clears +0x0C before the active FSM frame and
            // restores it on Finish/Kill; paused/disabled calls retain it.
            self.controller_flag_c = self.thinker.paused || self.thinker.disabled;
            for request in requests {
                match request {
                    Request::TargetPos { pos } => self.target = pos,
                    Request::Action { action } => {
                        self.effects.extend(
                            self.actor
                                .start_action(
                                    &self.data,
                                    action as u32,
                                    self.scale[1],
                                    HoverHeight::ThinkTarget,
                                )
                                .map_err(|e| format!("dragon action: {e:?}"))?,
                        );
                    }
                    Request::ActionEx {
                        action,
                        signed,
                        floats,
                        ..
                    } => {
                        let effects = match (action, signed[0]) {
                            // CtrlActionEx lanes become the action's +4 and
                            // +0x24 parameters before vf05 copies phase/delay
                            // (0x005A89E0 -> 0x00A6AED0). Opening mode is 0.
                            (33, 0) => self.actor.start_press(
                                &self.data, super::attacks::PressPhase::Waiting, floats[0]),
                            (54, 0) => self.actor.start_hover_kakku(
                                &self.data,
                                HoverKakkuTarget::ThinkTarget {
                                    position: self.target,
                                    speed: floats[0],
                                },
                            ),
                            (53, _) => self.actor.start_action(
                                &self.data,
                                53,
                                self.scale[1],
                                HoverHeight::ThinkTarget,
                            ),
                            (10, 0 | 2) => self.actor.start_target_fly(
                                &self.data,
                                if signed[0] == 0 {
                                    [floats[0], floats[1], floats[2]]
                                } else {
                                    self.target
                                },
                                floats[3],
                                floats[4],
                                floats[5],
                            ),
                            _ => {
                                return Err(format!(
                                    "dragon action {action}, mode {} unimplemented",
                                    signed[0]
                                ))
                            }
                        }
                        .map_err(|e| format!("dragon action: {e:?}"))?;
                        self.effects.extend(effects);
                    }
                    Request::FreeCondition { bits } => self.thinker.or_free_condition(bits),
                    other => self.world_requests.push(other),
                }
            }
        }
        if self.actor.motion().is_none() {
            return Ok(());
        }
        let transform =
            std::array::from_fn(|i| std::array::from_fn(|j| self.cached_matrix[i][j] * self.scale[i]));
        let (controller_flag_c, actor_flags) = self.collision_conditions
            .unwrap_or((self.controller_flag_c, self.condition_flags));
        self.effects.extend(
            self.actor
                .step(
                    &self.data,
                    DragonStep {
                        engine_step: step,
                        motion_step: step,
                        root_translation_transform: transform,
                        think_target_y: self.target[1],
                        motion_flags: 0,
                        yaw_enabled: true,
                        movement_condition_flags: 0,
                        yaw_slow_factor: 1.0,
                        external_displacement: [0.0; 3],
                        acceleration: [0.0; 3],
                        horizontal_status_multipliers: [None; 2],
                        // This entrance subset has no status acceleration or velocity
                        // constraints supplied by the host; do not tune arbitrary caps.
                        velocity_min: [f32::NEG_INFINITY; 3],
                        velocity_max: [f32::INFINITY; 3],
                        controller_flag_c,
                        actor_flags,
                        notify_attached: false,
                        attached_owners: &[],
                    },
                )
                .map_err(|e| format!("dragon phase A: {e:?}"))?,
        );
        Ok(())
    }

    /// After host terrain/push correction: phase 97 then volume activation.
    pub fn pass_c(&mut self, step: f32) -> Result<(), String> {
        self.finish_pass_c(step, None)
    }

    /// Actual host terrain, before collision activation. The current
    /// entrance flight path is unchanged; Press uses its installed capsule.
    pub fn pass_c_with_terrain(&mut self, step: f32, ground: &dyn ddsim::Ground) -> Result<(), String> {
        self.finish_pass_c(step, Some(ground))
    }

    fn finish_pass_c(&mut self, step: f32, ground: Option<&dyn ddsim::Ground>) -> Result<(), String> {
        if self.actor.motion().is_none() {
            return Ok(());
        }
        self.actor.pass_c_action(&self.cached_matrix, step);
        if self.actor.current_action() == Some(33) {
            let ground = ground.ok_or("Press requires the host's actual terrain")?;
            let pose = self.scaled_pose()?;
            let mut nodes = self.data.body.collision.iter().flat_map(|set| &set.body.groups)
                .filter(|group| group.is_scr_adj).flat_map(|group| &group.nodes);
            let node = nodes.next().ok_or("Dragon scenery capsule absent")?;
            if nodes.next().is_some() { return Err("multiple Dragon scenery nodes unported".into()); }
            let cap = super::attacks::scenery_capsule(node, &pose, self.actor.pos, self.scale[1])?;
            let start = [self.cached_matrix[3][0], self.cached_matrix[3][1], self.cached_matrix[3][2]];
            let mut motion = std::array::from_fn(|i| self.actor.pos[i] - start[i]);
            let grounded = self.actor.press.as_ref().is_some_and(|p| p.phase == super::attacks::PressPhase::Landed);
            if grounded {
                // cScrAdjust constructor 0x0079D850 initializes +0x190=-20,
                // +0x194=-5. 0x0079E200 adds this offset only in ground mode.
                // Runtime overrides of these constructor values are unported.
                motion[1] += if step >= f32::EPSILON { -20.0 * step } else { -5.0 };
            }
            let available = std::cell::Cell::new(true);
            let result = ddsim::sweep::sweep_capsule(&Default::default(), &cap, start, motion,
                grounded, None, &|lo, hi, out| {
                    if !ground.triangles_in(lo, hi, out) { available.set(false); }
                });
            if !available.get() { return Err("Press requires a triangle terrain provider".into()); }
            self.actor.pos = result.position;
            self.scenery_result = Some(result);
        }
        let pose = self.scaled_pose()?;
        self.actor
            .finish_pass_c(&self.data, &pose)
            .map_err(|e| format!("dragon pass C: {e:?}"))
    }

    fn scaled_pose(&self) -> Result<crate::body::Posed, String> {
        let mut pose = self
            .actor
            .pose(&self.data)
            .map_err(|e| format!("dragon pose: {e:?}"))?;
        // Scale belongs to the authored placement, not the mean volume scale.
        let model = mtlmt::pose::compose(self.scale, self.actor.rotation, self.actor.pos);
        let local = self.data.body.skeleton.pose(
            self.data.body.motion(self.actor.motion().unwrap()),
            self.actor.frame,
        );
        pose.model = model;
        pose.joints = self
            .data
            .body
            .skeleton
            .joints
            .iter()
            .zip(local)
            .map(|(joint, m)| (joint.id, mtlmt::pose::mul(&m, &model)))
            .collect();
        Ok(pose)
    }
}
