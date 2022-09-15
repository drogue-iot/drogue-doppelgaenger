use crate::model::Thing;
use drogue_doppelgaenger_model::InternalState;
use std::fmt::{Display, Formatter};

#[derive(
    Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Hash, serde::Deserialize, serde::Serialize,
)]
pub struct Id {
    pub application: String,
    pub thing: String,
}

impl Id {
    pub fn new<A: Into<String>, T: Into<String>>(application: A, thing: T) -> Self {
        Self {
            application: application.into(),
            thing: thing.into(),
        }
    }

    pub fn make_thing<I: InternalState>(&self) -> Thing<I> {
        Thing::new(&self.application, &self.thing)
    }
}

impl Display for Id {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.application, self.thing)
    }
}

impl<A: Into<String>, T: Into<String>> From<(A, T)> for Id {
    fn from((application, thing): (A, T)) -> Self {
        Id::new(application, thing)
    }
}
