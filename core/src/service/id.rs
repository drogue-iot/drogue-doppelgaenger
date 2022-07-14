use std::fmt::{Display, Formatter};

#[derive(
    Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Hash, serde::Deserialize, serde::Serialize,
)]
pub struct Id {
    pub application: String,
    pub thing: String,
}

impl Display for Id {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.application, self.thing)
    }
}
