use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct $name(pub String);

        impl $name {
            pub fn new() -> Self {
                static NEXT_ID: AtomicU64 = AtomicU64::new(1);
                let value = NEXT_ID.fetch_add(1, Ordering::Relaxed);
                Self(format!("{}-{value}", stringify!($name)))
            }

            pub fn from_string(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }
    };
}

id_type!(ActionId);
id_type!(AgentId);
id_type!(DecisionId);
id_type!(IncidentId);
id_type!(MessageId);
id_type!(ObservationId);
id_type!(ProposalId);
id_type!(RecommendationId);
id_type!(TraceId);
