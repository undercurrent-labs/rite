use crate::registry::NativeFunctionDescriptor;
use indexmap::IndexMap;
use rite_runtime::{EvalError, Key, Value};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemDef {
    pub id: String,
    pub name: String,
    pub weight: i64,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomDef {
    pub id: String,
    pub text: String,
    pub exits: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    pub location: String,
    pub inventory: Vec<String>,
    pub flags: HashSet<String>,
    pub revealed: HashSet<String>,
    pub messages: Vec<String>,
}

pub struct GameCap {
    pub items: HashMap<String, ItemDef>,
    pub rooms: HashMap<String, RoomDef>,
    pub state: GameState,
    pub started: bool,
}

impl GameCap {
    pub fn new() -> Self {
        Self {
            items: HashMap::new(),
            rooms: HashMap::new(),
            state: GameState {
                location: String::new(),
                inventory: Vec::new(),
                flags: HashSet::new(),
                revealed: HashSet::new(),
                messages: Vec::new(),
            },
            started: false,
        }
    }

    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "register_item",
            docs: "Register an item entity.",
            arity: 2,
            effectful: true,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "register_room",
            docs: "Register a room entity.",
            arity: 2,
            effectful: true,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "register_world",
            docs: "Register world metadata.",
            arity: 2,
            effectful: true,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "say",
            docs: "Emit a narrative message.",
            arity: 1,
            effectful: true,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "reveal",
            docs: "Reveal a room or flag.",
            arity: 1,
            effectful: true,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "go",
            docs: "Move through an exit.",
            arity: 1,
            effectful: true,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "take",
            docs: "Add item to inventory.",
            arity: 1,
            effectful: true,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "drop",
            docs: "Remove item from inventory.",
            arity: 1,
            effectful: true,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "look",
            docs: "Describe current room.",
            arity: 0,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "inventory",
            docs: "List inventory item ids.",
            arity: 0,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "save",
            docs: "Serialize game state to JSON string.",
            arity: 0,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "load",
            docs: "Load game state from JSON string.",
            arity: 1,
            effectful: true,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "start",
            docs: "Start game at a room.",
            arity: 1,
            effectful: true,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "command",
            docs: "Parse and run a player command.",
            arity: 1,
            effectful: true,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "messages",
            docs: "Drain pending narrative messages.",
            arity: 0,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "state",
            docs: "Return current game state record.",
            arity: 0,
            effectful: false,
            permission: "",
        },
    ];

    pub fn call(&mut self, method: &str, args: Vec<Value>) -> Result<Value, EvalError> {
        match method {
            "register_item" => {
                let id = atom_or_str(args.first())?;
                let mut name = id.clone();
                let mut weight = 1i64;
                let mut tags = Vec::new();
                // Body is closure - extract not available; accept optional record as 2nd arg
                if let Some(Value::Record(r)) = args.get(1) {
                    if let Some(Value::String(s)) = r.get(&Key::String("name".into())) {
                        name = s.to_string();
                    }
                    if let Some(Value::Int(w)) = r.get(&Key::String("weight".into())) {
                        weight = *w;
                    }
                    if let Some(Value::List(xs)) = r.get(&Key::String("tags".into())) {
                        tags = xs
                            .iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect();
                    }
                }
                if self.items.contains_key(&id) {
                    return Err(EvalError::Message(format!("duplicate item `{}`", id)));
                }
                self.items.insert(
                    id.clone(),
                    ItemDef {
                        id,
                        name,
                        weight,
                        tags,
                    },
                );
                Ok(Value::None)
            }
            "register_room" => {
                let id = atom_or_str(args.first())?;
                let mut text = String::new();
                let mut exits = HashMap::new();
                if let Some(Value::Record(r)) = args.get(1) {
                    if let Some(Value::String(s)) = r.get(&Key::String("text".into())) {
                        text = s.to_string();
                    }
                    if let Some(Value::Record(ex)) = r.get(&Key::String("exits".into())) {
                        for (k, v) in ex {
                            exits.insert(k.as_str(), atom_or_str(Some(v)).unwrap_or_default());
                        }
                    }
                }
                // Also support string second arg as text
                if let Some(Value::String(s)) = args.get(1) {
                    text = s.to_string();
                }
                if self.rooms.contains_key(&id) {
                    return Err(EvalError::Message(format!("duplicate room `{}`", id)));
                }
                self.rooms.insert(
                    id.clone(),
                    RoomDef {
                        id: id.clone(),
                        text,
                        exits,
                    },
                );
                if self.state.location.is_empty() {
                    self.state.location = id;
                }
                Ok(Value::None)
            }
            "register_world" => Ok(Value::None),
            "say" => {
                let msg = args.first().map(|v| format!("{}", v)).unwrap_or_default();
                self.state.messages.push(msg);
                Ok(Value::None)
            }
            "reveal" => {
                let id = atom_or_str(args.first())?;
                self.state.revealed.insert(id);
                Ok(Value::None)
            }
            "go" => {
                let dir = atom_or_str(args.first())?;
                let room = self
                    .rooms
                    .get(&self.state.location)
                    .ok_or_else(|| EvalError::Message("not in a room".into()))?;
                if let Some(dest) = room.exits.get(&dir).cloned() {
                    // Check if dest is revealed or always open
                    if !self.rooms.contains_key(&dest) && !self.state.revealed.contains(&dest) {
                        self.state.messages.push("You cannot go that way.".into());
                        return Ok(Value::Bool(false));
                    }
                    self.state.location = dest;
                    if let Some(r) = self.rooms.get(&self.state.location) {
                        self.state.messages.push(r.text.clone());
                    }
                    Ok(Value::Bool(true))
                } else {
                    self.state
                        .messages
                        .push("There is no exit that way.".into());
                    Ok(Value::Bool(false))
                }
            }
            "take" => {
                let id = atom_or_str(args.first())?;
                if !self.state.inventory.contains(&id) {
                    self.state.inventory.push(id.clone());
                    self.state.messages.push(format!("Taken: {}", id));
                }
                Ok(Value::None)
            }
            "drop" => {
                let id = atom_or_str(args.first())?;
                self.state.inventory.retain(|x| x != &id);
                Ok(Value::None)
            }
            "look" => {
                if let Some(r) = self.rooms.get(&self.state.location) {
                    Ok(Value::string(r.text.clone()))
                } else {
                    Ok(Value::string("Darkness."))
                }
            }
            "inventory" => Ok(Value::list(
                self.state
                    .inventory
                    .iter()
                    .map(|s| Value::string(s.clone()))
                    .collect::<Vec<_>>(),
            )),
            "save" => {
                let json = serde_json::to_string(&self.state)
                    .map_err(|e| EvalError::Capability(e.to_string()))?;
                Ok(Value::ok(Value::string(json)))
            }
            "load" => {
                let s = args
                    .first()
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| EvalError::Message("game.load expects string".into()))?;
                match serde_json::from_str::<GameState>(s) {
                    Ok(st) => {
                        self.state = st;
                        Ok(Value::ok(Value::None))
                    }
                    Err(e) => Ok(Value::err(Value::string(e.to_string()))),
                }
            }
            "start" => {
                let room = atom_or_str(args.first())?;
                self.state.location = room;
                self.started = true;
                if let Some(r) = self.rooms.get(&self.state.location) {
                    self.state.messages.push(r.text.clone());
                }
                Ok(Value::None)
            }
            "command" => {
                let cmd = args
                    .first()
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_lowercase();
                let parts: Vec<&str> = cmd.split_whitespace().collect();
                match parts.as_slice() {
                    ["look"] | ["l"] => {
                        let desc = self.call("look", vec![])?;
                        if let Value::String(s) = desc {
                            self.state.messages.push(s.to_string());
                        }
                    }
                    ["inventory"] | ["i"] => {
                        let inv = self.state.inventory.join(", ");
                        self.state.messages.push(if inv.is_empty() {
                            "You carry nothing.".into()
                        } else {
                            format!("You carry: {}", inv)
                        });
                    }
                    ["go", dir] | ["move", dir] => {
                        let _ = self.call("go", vec![Value::string(*dir)])?;
                    }
                    ["north"] | ["n"] => {
                        let _ = self.call("go", vec![Value::string("north")])?;
                    }
                    ["south"] | ["s"] => {
                        let _ = self.call("go", vec![Value::string("south")])?;
                    }
                    ["east"] | ["e"] => {
                        let _ = self.call("go", vec![Value::string("east")])?;
                    }
                    ["west"] | ["w"] => {
                        let _ = self.call("go", vec![Value::string("west")])?;
                    }
                    ["take", item] | ["get", item] => {
                        let _ = self.call("take", vec![Value::string(*item)])?;
                    }
                    ["drop", item] => {
                        let _ = self.call("drop", vec![Value::string(*item)])?;
                    }
                    ["quit"] | ["exit"] => {
                        self.state.messages.push("Farewell.".into());
                    }
                    _ => {
                        self.state
                            .messages
                            .push(format!("Unknown command: {}", cmd));
                    }
                }
                Ok(Value::None)
            }
            "messages" => {
                let msgs: Vec<Value> = self.state.messages.drain(..).map(Value::string).collect();
                Ok(Value::list(msgs))
            }
            "state" => {
                let mut rec = IndexMap::new();
                rec.insert(
                    Key::String("location".into()),
                    Value::string(self.state.location.clone()),
                );
                rec.insert(
                    Key::String("inventory".into()),
                    Value::list(
                        self.state
                            .inventory
                            .iter()
                            .map(|s| Value::string(s.clone()))
                            .collect::<Vec<_>>(),
                    ),
                );
                Ok(Value::Record(rec))
            }
            other => Err(EvalError::Capability(format!("unknown @game.{}", other))),
        }
    }
}

fn atom_or_str(v: Option<&Value>) -> Result<String, EvalError> {
    match v {
        Some(Value::String(s)) => Ok(s.to_string()),
        Some(Value::Atom(id)) => Ok(format!("atom_{}", id.0)),
        Some(other) => {
            let s = format!("{}", other);
            Ok(s.trim_start_matches('#').to_string())
        }
        None => Err(EvalError::Message("expected atom or string".into())),
    }
}

impl Default for GameCap {
    fn default() -> Self {
        Self::new()
    }
}
