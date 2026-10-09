//! `rAIGoalPlanning`: one set of goals and the actions that reach them.

use crate::{class, int, list, string};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Goal {
    /// `GoalName`, e.g. `NormalAttack`.
    pub name: String,
    /// `PremiseConditionList[].PremiseCondition`: premise numbers.
    pub premises: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    /// `PremiseCondition`: the premise the action needs.
    pub premise: i64,
    /// `Effect`: the premise it establishes.
    pub effect: i64,
    /// `InterfaceID`: the action interface number.
    pub interface_id: i64,
    /// Class of `Interface`, e.g. `cCmcSwordCombo` (`None` if absent).
    pub class: Option<String>,
    pub element_attr: i64,
    pub atk_attr: i64,
    pub use_attr: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalPlanning {
    /// `PremiseConditionList[].条件` (condition numbers).
    pub premises: Vec<i64>,
    pub goals: Vec<Goal>,
    pub actions: Vec<Action>,
}

impl GoalPlanning {
    pub fn parse(bytes: &[u8]) -> Result<GoalPlanning, String> {
        let x = mtxfs::parse(bytes).map_err(|e| format!("{e:?}"))?;
        let r = &x.root;
        if class(&x, r) != "rAIGoalPlanning" {
            return Err(format!("root is {}", class(&x, r)));
        }
        let premises = list(&x, r, "PremiseConditionList")?
            .into_iter()
            .map(|p| int(&x, p, "条件"))
            .collect::<Result<_, _>>()?;
        let goals = list(&x, r, "GoalList")?
            .into_iter()
            .map(|g| {
                Ok(Goal {
                    name: string(&x, g, "GoalName")?,
                    premises: list(&x, g, "PremiseConditionList")?
                        .into_iter()
                        .map(|p| int(&x, p, "PremiseCondition"))
                        .collect::<Result<_, _>>()?,
                })
            })
            .collect::<Result<_, String>>()?;
        let actions = list(&x, r, "ActionInterfaceList")?
            .into_iter()
            .map(|a| {
                Ok(Action {
                    premise: int(&x, a, "PremiseCondition")?,
                    effect: int(&x, a, "Effect")?,
                    interface_id: int(&x, a, "InterfaceID")?,
                    class: list(&x, a, "Interface")?.first().map(|o| class(&x, o)),
                    element_attr: int(&x, a, "ElementAttr")?,
                    atk_attr: int(&x, a, "AtkAttr")?,
                    use_attr: int(&x, a, "UseAttr")?,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(GoalPlanning { premises, goals, actions })
    }
}
