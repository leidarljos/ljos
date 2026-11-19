//! The shapes a few write tools answer with.
//!
//! The pack hands back the atom it stored, and a user rule is a row the
//! seat keeps itself. A tool passes that object through as it came and
//! lists its fields here, so a client reads `outputSchema` instead of
//! guessing at an open object. Every field a result can carry is named;
//! only `id`, `text` and the fields a tool always sets are required.

use std::borrow::Cow;
use std::marker::PhantomData;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::Serialize;
use serde_json::{Map, Value};

/// An answer sent as it came, under the schema of the shape `T` it has.
pub struct Shaped<T> {
    body: Map<String, Value>,
    shape: PhantomData<fn() -> T>,
}

impl<T> Serialize for Shaped<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.body.serialize(serializer)
    }
}

impl<T: JsonSchema> JsonSchema for Shaped<T> {
    fn inline_schema() -> bool {
        T::inline_schema()
    }

    fn schema_name() -> Cow<'static, str> {
        T::schema_name()
    }

    fn schema_id() -> Cow<'static, str> {
        T::schema_id()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        T::json_schema(generator)
    }
}

/// A writer's answer as the object a structured result has to be: the
/// object itself, or any other value carried under `value`.
pub fn shaped<T>(value: Value) -> rmcp::Json<Shaped<T>> {
    let body = match value {
        Value::Object(map) => map,
        other => {
            let mut map = Map::new();
            map.insert("value".into(), other);
            map
        }
    };
    rmcp::Json(Shaped {
        body,
        shape: PhantomData,
    })
}

/// Where an atom came from.
#[derive(Serialize, JsonSchema)]
pub struct AtomSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub harness: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub seat: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub via: Option<String>,
    /// Whose words the text is: `person` or `agent`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub basis: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub citation: Option<String>,
}

/// The writing measures the pack took of the text.
#[derive(Serialize, JsonSchema)]
pub struct AtomProse {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u32")]
    pub words: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u32")]
    pub sentences: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ease: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u32")]
    pub adverbs: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub adverb_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u32")]
    pub passives: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u32")]
    pub hard_sentences: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u32")]
    pub very_hard_sentences: Option<u32>,
}

/// When the atom is next due for review.
#[derive(Serialize, JsonSchema)]
pub struct AtomReview {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u32")]
    pub reps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u64")]
    pub interval_s: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub ease: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub stability: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub difficulty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub last: Option<String>,
}

/// The fields every stored atom has, whatever its kind.
#[derive(Serialize, JsonSchema)]
pub struct AtomFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub schema: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub workspace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<String>")]
    pub entities: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "AtomSource")]
    pub source: Option<AtomSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub origin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "AtomProse")]
    pub prose: Option<AtomProse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "AtomReview")]
    pub review: Option<AtomReview>,
    /// RFC 3339.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub ts: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub valid_from: Option<String>,
    /// Set when the atom was retired.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub valid_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub updated: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub due_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub tombstone: Option<bool>,
    /// The ids of the atoms this one replaced.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<String>")]
    pub supersedes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<Value>")]
    pub links: Option<Vec<Value>>,
}

/// Handles and names a user rule refuses.
#[derive(Serialize, JsonSchema)]
pub struct People {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<String>")]
    pub handles: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<String>")]
    pub names: Option<Vec<String>>,
}

/// The fields a user rule has beyond an atom's.
#[derive(Serialize, JsonSchema)]
pub struct UserRuleFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub written_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub updated_at: Option<String>,
    /// True once the rule was removed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub deleted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "People")]
    pub deny_mention: Option<People>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "People")]
    pub deny_reply_to: Option<People>,
    /// The lesson a promote copied.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub basis: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub lesson_hash: Option<String>,
}

/// What `ljos_rule` returns: the stored rule, a seat atom or a user rule.
#[derive(Serialize, JsonSchema)]
pub struct RuleOut {
    pub id: String,
    pub text: String,
    pub pattern: String,
    /// `deny` or `ask`.
    pub verdict: String,
    #[serde(flatten)]
    pub atom: AtomFields,
    #[serde(flatten)]
    pub user: UserRuleFields,
}

/// What `ljos_prefer` returns: the stored preference, the user rule it
/// wrote, promoted or removed, or the report of a sync.
#[derive(Serialize, JsonSchema)]
pub struct PreferOut {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub verdict: Option<String>,
    /// What a sync pulled and pushed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub report: Option<String>,
    #[serde(flatten)]
    pub atom: AtomFields,
    #[serde(flatten)]
    pub user: UserRuleFields,
}

/// What `ljos_forget` returns: the retired atom.
#[derive(Serialize, JsonSchema)]
pub struct ForgetOut {
    pub id: String,
    pub text: String,
    #[serde(flatten)]
    pub atom: AtomFields,
}

/// The reading a habit atom carries.
#[derive(Serialize, JsonSchema)]
pub struct HabitReading {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub source: Option<String>,
    /// Seconds between readings.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u64")]
    pub every_s: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub was: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub was_ts: Option<String>,
}

/// The habit atom `ljos_habit` stored.
#[derive(Serialize, JsonSchema)]
pub struct HabitAtom {
    pub id: String,
    pub text: String,
    pub habit: HabitReading,
    #[serde(flatten)]
    pub atom: AtomFields,
}

/// The reading a new one closed.
#[derive(Serialize, JsonSchema)]
pub struct HabitWas {
    pub value: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub ts: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub id: Option<String>,
}

/// What `ljos_habit` returns. With a value: `reading`, and `was` when an
/// earlier reading was closed. Without one: `value`, the habits as they
/// stand.
#[derive(Serialize, JsonSchema)]
pub struct HabitOut {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "HabitAtom")]
    pub reading: Option<HabitAtom>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub was: Option<HabitWas>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<crate::server::ReadingRow>")]
    pub value: Option<Vec<crate::server::ReadingRow>>,
}

/// What `ljos_predict` returns: the forecast atom.
#[derive(Serialize, JsonSchema)]
pub struct PredictOut {
    pub id: String,
    pub text: String,
    pub issue: String,
    pub agent: String,
    /// The option expected to win, or an object of option to share.
    pub expect: Value,
    #[serde(flatten)]
    pub atom: AtomFields,
}

/// What `ljos_trust` returns: the trust atom.
#[derive(Serialize, JsonSchema)]
pub struct TrustOut {
    pub id: String,
    pub text: String,
    pub from: String,
    pub to: String,
    pub weight: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<String>")]
    pub about: Option<Vec<String>>,
    #[serde(flatten)]
    pub atom: AtomFields,
}

/// What `ljos_persona` returns: the persona atom.
#[derive(Serialize, JsonSchema)]
pub struct PersonaOut {
    pub id: String,
    pub text: String,
    pub name: String,
    pub anchor: f64,
    /// The seat that defined the persona.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub holder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub runner: Option<String>,
    #[serde(flatten)]
    pub atom: AtomFields,
}
