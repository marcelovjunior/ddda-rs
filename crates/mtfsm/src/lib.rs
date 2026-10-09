//! Typed rAIFSM graph over XFS. This is a data reader, not an AI interpreter.
//! Preserves ordered lists, raw operator codes and heterogeneous parameter fields.
use mtxfs::{Object, Value, Xfs};

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub class_hash: u32,
    pub class_name: Option<&'static str>,
    pub fields: Vec<(String, Vec<Parameter>)>,
}
#[derive(Debug, Clone, PartialEq)]
pub enum Parameter {
    Scalar(Value),
    Object(Option<Box<Record>>),
}
#[derive(Debug, Clone, PartialEq)]
pub struct Process {
    /// Process dispatch uses names, not a serialized numeric opcode.
    pub container: Vec<u8>,
    pub category: Vec<u8>,
    pub parameter: Option<Record>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Expression {
    pub record: Record,
    pub operator: Option<u32>,
    pub children: Vec<Option<Expression>>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    pub id: u32,
    pub name: Vec<u8>,
    pub root: Option<Expression>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub name: Vec<u8>,
    pub destination: u32,
    pub enabled: bool,
    pub condition: u32,
}
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub id: u32,
    pub unique_id: u32,
    pub owner_id: u32,
    pub name: Vec<u8>,
    pub setting: u32,
    pub user_attribute: u32,
    pub from_all: Option<u32>,
    pub links: Vec<Link>,
    pub processes: Vec<Process>,
    pub subcluster: Option<Box<Cluster>>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Cluster {
    pub id: u32,
    pub owner_node_unique_id: u32,
    pub initial_state: u32,
    pub states: Vec<State>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Graph {
    pub owner: Vec<u8>,
    pub attribute: u32,
    pub root: Option<Cluster>,
    pub conditions: Vec<Condition>,
}
type Result<T> = std::result::Result<T, String>;
fn values<'a>(x: &Xfs, o: &'a Object, n: &str) -> Result<&'a [Value]> {
    x.get(o, n)
        .ok_or_else(|| format!("{} missing {n}", x.class_of(o).name().unwrap_or("?")))
}
fn one<'a>(x: &Xfs, o: &'a Object, n: &str) -> Result<&'a Value> {
    match values(x, o, n)? {
        [v] => Ok(v),
        _ => Err(format!("{n}: expected one value")),
    }
}
fn uint(x: &Xfs, o: &Object, n: &str) -> Result<u32> {
    match one(x, o, n)? {
        Value::U(v) => u32::try_from(*v).map_err(|_| format!("{n}: out of range")),
        _ => Err(format!("{n}: expected u32")),
    }
}
fn boolean(x: &Xfs, o: &Object, n: &str) -> Result<bool> {
    match one(x, o, n)? {
        Value::Bool(v) => Ok(*v),
        _ => Err(format!("{n}: expected bool")),
    }
}
fn string(x: &Xfs, o: &Object, n: &str) -> Result<Vec<u8>> {
    match one(x, o, n)? {
        Value::Str(v) => Ok(v.clone()),
        _ => Err(format!("{n}: expected string")),
    }
}
fn optional<'a>(x: &Xfs, o: &'a Object, n: &str) -> Result<Option<&'a Object>> {
    match one(x, o, n)? {
        Value::Object(v) => Ok(v.as_deref()),
        _ => Err(format!("{n}: expected object")),
    }
}
fn objects<'a>(x: &Xfs, o: &'a Object, n: &str) -> Result<Vec<&'a Object>> {
    values(x, o, n)?
        .iter()
        .map(|v| match v {
            Value::Object(Some(v)) => Ok(v.as_ref()),
            _ => Err(format!("{n}: expected non-null object")),
        })
        .collect()
}
fn class(x: &Xfs, o: &Object, n: &str) -> Result<()> {
    if x.class_of(o).name() == Some(n) {
        Ok(())
    } else {
        Err(format!("expected {n}, got {:?}", x.class_of(o).name()))
    }
}
fn record(x: &Xfs, o: &Object) -> Record {
    let c = x.class_of(o);
    Record {
        class_hash: c.hash,
        class_name: c.name(),
        fields: c
            .properties
            .iter()
            .zip(&o.fields)
            .map(|(p, vs)| {
                (
                    p.name.clone(),
                    vs.iter()
                        .map(|v| match v {
                            Value::Object(o) => Parameter::Object(o.as_ref().map(|o| Box::new(record(x, o)))),
                            _ => Parameter::Scalar(v.clone()),
                        })
                        .collect(),
                )
            })
            .collect(),
    }
}
fn expression(x: &Xfs, o: &Object) -> Result<Expression> {
    let operator = if x.class_of(o).name() == Some("rAIConditionTree::OperationNode") {
        Some(uint(x, o, "mOperator")?)
    } else {
        None
    };
    Ok(Expression {
        record: record(x, o),
        operator,
        children: values(x, o, "mpChildList")?
            .iter()
            .map(|v| match v {
                Value::Object(o) => o.as_deref().map(|o| expression(x, o)).transpose(),
                _ => Err("mpChildList: expected object".into()),
            })
            .collect::<Result<_>>()?,
    })
}
fn cluster(x: &Xfs, o: &Object) -> Result<Cluster> {
    class(x, o, "cAIFSMCluster")?;
    let mut states = Vec::new();
    for s in objects(x, o, "mpNodeList")? {
        class(x, s, "cAIFSMNode")?;
        let links = objects(x, s, "mpLinkList")?
            .into_iter()
            .map(|l| {
                class(x, l, "cAIFSMLink")?;
                Ok(Link {
                    name: string(x, l, "mName")?,
                    destination: uint(x, l, "mDestinationNodeId")?,
                    enabled: boolean(x, l, "mExistCondition")?,
                    condition: uint(x, l, "mConditionId")?,
                })
            })
            .collect::<Result<_>>()?;
        let processes = objects(x, s, "mpProcessList")?
            .into_iter()
            .map(|p| {
                class(x, p, "cAIFSMNodeProcess")?;
                Ok(Process {
                    container: string(x, p, "mContainerName")?,
                    category: string(x, p, "mCategoryName")?,
                    parameter: optional(x, p, "mpParameter")?.map(|p| record(x, p)),
                })
            })
            .collect::<Result<_>>()?;
        states.push(State {
            id: uint(x, s, "mId")?,
            unique_id: uint(x, s, "mUniqueId")?,
            owner_id: uint(x, s, "mOwnerId")?,
            name: string(x, s, "mName")?,
            setting: uint(x, s, "mSetting")?,
            user_attribute: uint(x, s, "mUserAttribute")?,
            from_all: if boolean(x, s, "mExistConditionTrainsitionFromAll")? {
                Some(uint(x, s, "mConditionTrainsitionFromAllId")?)
            } else {
                None
            },
            links,
            processes,
            subcluster: optional(x, s, "mpSubCluster")?
                .map(|o| cluster(x, o).map(Box::new))
                .transpose()?,
        });
    }
    Ok(Cluster {
        id: uint(x, o, "mId")?,
        owner_node_unique_id: uint(x, o, "mOwnerNodeUniqueId")?,
        initial_state: uint(x, o, "mInitialStateId")?,
        states,
    })
}
impl Graph {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        Self::from_xfs(&mtxfs::parse(bytes).map_err(|e| e.to_string())?)
    }
    pub fn from_xfs(x: &Xfs) -> Result<Self> {
        class(x, &x.root, "rAIFSM")?;
        let mut conditions = Vec::new();
        if let Some(t) = optional(x, &x.root, "mpConditionTree")? {
            class(x, t, "rAIConditionTree")?;
            for c in objects(x, t, "mpTreeList")? {
                class(x, c, "rAIConditionTree::TreeInfo")?;
                let n = optional(x, c, "mName")?.ok_or("null condition name")?;
                conditions.push(Condition {
                    id: uint(x, n, "mId")?,
                    name: string(x, n, "mElementName")?,
                    root: optional(x, c, "mpRootNode")?
                        .map(|o| expression(x, o))
                        .transpose()?,
                });
            }
        }
        let graph = Self {
            owner: string(x, &x.root, "mOwnerObjectName")?,
            attribute: uint(x, &x.root, "mFSMAttribute")?,
            root: optional(x, &x.root, "mpRootCluster")?
                .map(|o| cluster(x, o))
                .transpose()?,
            conditions,
        };
        graph.validate()?;
        Ok(graph)
    }
    /// First matching condition, matching the executable's ordered lookup.
    // Translated from 0x0117aaa0 (DDDA.exe 2017-12-13 Steam): first matching id.
    pub fn condition(&self, id: u32) -> Option<&Condition> {
        self.conditions.iter().find(|c| c.id == id)
    }
    /// Rejects dangling active links and initial states. Disabled links retain
    /// their serialized values (including 0xffffffff) and are never traversed.
    pub fn validate(&self) -> Result<()> {
        fn visit(g: &Graph, c: &Cluster) -> Result<()> {
            if c.state(c.initial_state).is_none() {
                return Err(format!(
                    "cluster {}: absent initial state {}",
                    c.id, c.initial_state
                ));
            }
            for s in &c.states {
                if let Some(id) = s.from_all {
                    if g.condition(id).is_none() {
                        return Err(format!("state {}: absent entry condition {id}", s.id));
                    }
                }
                for l in &s.links {
                    if l.enabled {
                        if c.state(l.destination).is_none() {
                            return Err(format!("state {}: absent destination {}", s.id, l.destination));
                        }
                        if g.condition(l.condition).is_none() {
                            return Err(format!("state {}: absent condition {}", s.id, l.condition));
                        }
                    }
                }
                if let Some(c) = &s.subcluster {
                    visit(g, c)?;
                }
            }
            Ok(())
        }
        if let Some(c) = &self.root {
            visit(self, c)?;
        }
        Ok(())
    }
}
impl Cluster {
    // Translated from 0x010c9290 (DDDA.exe 2017-12-13 Steam): first matching id.
    pub fn state(&self, id: u32) -> Option<&State> {
        self.states.iter().find(|s| s.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(id: u32) -> State {
        State {
            id,
            unique_id: id,
            owner_id: 0,
            name: vec![],
            setting: 0,
            user_attribute: 0,
            from_all: None,
            links: vec![],
            processes: vec![],
            subcluster: None,
        }
    }
    fn graph() -> Graph {
        Graph {
            owner: vec![],
            attribute: 0,
            root: Some(Cluster {
                id: 0,
                owner_node_unique_id: 0,
                initial_state: 7,
                states: vec![state(7), state(9)],
            }),
            conditions: vec![Condition {
                id: 3,
                name: b"first".to_vec(),
                root: None,
            }],
        }
    }
    #[test]
    fn active_edges_require_state_and_condition() {
        let mut g = graph();
        g.root.as_mut().unwrap().states[0].links.push(Link {
            name: vec![],
            destination: 9,
            enabled: true,
            condition: 3,
        });
        assert!(g.validate().is_ok());
        g.root.as_mut().unwrap().states[0].links[0].destination = 99;
        assert!(g.validate().unwrap_err().contains("destination"));
        g.root.as_mut().unwrap().states[0].links[0].destination = 9;
        g.root.as_mut().unwrap().states[0].links[0].condition = 99;
        assert!(g.validate().unwrap_err().contains("condition"));
    }
    #[test]
    fn disabled_sentinel_is_retained() {
        let mut g = graph();
        g.root.as_mut().unwrap().states[0].links.push(Link {
            name: vec![],
            destination: u32::MAX,
            enabled: false,
            condition: u32::MAX,
        });
        assert!(g.validate().is_ok());
        assert_eq!(g.root.unwrap().states[0].links[0].destination, u32::MAX);
    }
    #[test]
    fn initial_entry_and_nested_references_checked() {
        let mut g = graph();
        g.root.as_mut().unwrap().initial_state = 88;
        assert!(g.validate().unwrap_err().contains("initial"));
        g.root.as_mut().unwrap().initial_state = 7;
        g.root.as_mut().unwrap().states[0].from_all = Some(99);
        assert!(g.validate().unwrap_err().contains("entry condition"));
        g.root.as_mut().unwrap().states[0].from_all = Some(3);
        let mut child = g.root.clone().unwrap();
        child.initial_state = 88;
        g.root.as_mut().unwrap().states[0].subcluster = Some(Box::new(child));
        assert!(g.validate().unwrap_err().contains("initial"));
    }
    #[test]
    fn first_duplicate_id_wins() {
        let mut g = graph();
        g.conditions.push(Condition {
            id: 3,
            name: b"second".to_vec(),
            root: None,
        });
        let mut s = state(7);
        s.name = b"second".to_vec();
        g.root.as_mut().unwrap().states.push(s);
        assert_eq!(g.condition(3).unwrap().name, b"first");
        assert!(g.root.as_ref().unwrap().state(7).unwrap().name.is_empty());
    }
    fn obj(x: &mut Xfs, name: &str, fields: Vec<(&str, Vec<Value>)>) -> Object {
        let class = x.classes.len();
        x.classes.push(mtxfs::ClassDef {
            hash: mtarc::type_hash(name),
            engine_value: 0,
            properties: fields
                .iter()
                .map(|(name, _)| mtxfs::Property {
                    name: name.to_string(),
                    ty: mtxfs::Type::ClassRef,
                    attr: 0,
                    size: 4,
                })
                .collect(),
        });
        Object {
            class,
            number: 0,
            fields: fields.into_iter().map(|(_, v)| v).collect(),
        }
    }
    fn boxed(o: Object) -> Value {
        Value::Object(Some(Box::new(o)))
    }
    #[test]
    fn typed_projection_preserves_null_operand_unknown_operator_and_parameters() {
        let mut x = Xfs {
            minor: 2,
            classes: vec![],
            root: Object {
                class: 0,
                number: 0,
                fields: vec![],
            },
            object_count: 0,
        };
        let expr = obj(
            &mut x,
            "rAIConditionTree::OperationNode",
            vec![
                ("mOperator", vec![Value::U(99)]),
                ("mpChildList", vec![Value::Object(None)]),
            ],
        );
        let parsed = expression(&x, &expr).unwrap();
        assert_eq!(parsed.operator, Some(99));
        assert_eq!(parsed.children, vec![None]);
        let parameter = obj(
            &mut x,
            "cAICopiableParameter",
            vec![(
                "synthetic",
                vec![Value::S(-7), Value::F(vec![2.5]), Value::Str(vec![0xff])],
            )],
        );
        let saved = record(&x, &parameter);
        assert_eq!(saved.fields[0].1[2], Parameter::Scalar(Value::Str(vec![0xff])));
        let name = obj(
            &mut x,
            "cAIDEnum",
            vec![
                ("mId", vec![Value::U(12)]),
                ("mElementName", vec![Value::Str(b"synthetic".to_vec())]),
            ],
        );
        let info = obj(
            &mut x,
            "rAIConditionTree::TreeInfo",
            vec![("mName", vec![boxed(name)]), ("mpRootNode", vec![boxed(expr)])],
        );
        let tree = obj(
            &mut x,
            "rAIConditionTree",
            vec![("mpTreeList", vec![boxed(info)])],
        );
        x.root = obj(
            &mut x,
            "rAIFSM",
            vec![
                ("mOwnerObjectName", vec![Value::Str(b"synthetic".to_vec())]),
                ("mFSMAttribute", vec![Value::U(3)]),
                ("mpRootCluster", vec![Value::Object(None)]),
                ("mpConditionTree", vec![boxed(tree)]),
            ],
        );
        let g = Graph::from_xfs(&x).unwrap();
        assert_eq!(g.conditions[0].id, 12);
        assert_eq!(g.conditions[0].root.as_ref().unwrap().children, vec![None]);
        x.root.fields[1] = vec![Value::S(-1)];
        assert!(Graph::from_xfs(&x).is_err());
    }
    #[test]
    fn bad_binary_rejected() {
        assert!(Graph::parse(b"not XFS").is_err());
    }
}
