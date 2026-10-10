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
    /// Condition projection is accepted only when the host has real actor
    /// and controller words. Missing state preserves initialized rates.
    pub collision_conditions: Option<(bool, u32)>,
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
        // If condition/controller integration is absent, do not fabricate
        // it to remove the sequence-4 initial HP protection.
        let (controller_flag_c, actor_flags) = self.collision_conditions.unwrap_or((true, 0));
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
        if self.actor.motion().is_none() {
            return Ok(());
        }
        self.actor.pass_c_action(&self.cached_matrix, step);
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
        self.actor
            .finish_pass_c(&self.data, &pose)
            .map_err(|e| format!("dragon pass C: {e:?}"))
    }
}
