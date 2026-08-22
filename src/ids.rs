//! Compile-time safe identifiers for Herdr resources.
//!
//! Each ID is a distinct newtype around the wire string, so a `PaneId` can
//! never be passed where a `WorkspaceId` is expected. `#[serde(transparent)]`
//! keeps the JSON protocol identical to plain strings.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub(crate) struct $name(String);

        impl $name {
            pub(crate) fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_type!(
    /// A Herdr workspace id, e.g. `w3`.
    WorkspaceId
);
id_type!(
    /// A Herdr tab id, e.g. `w3:t1`.
    TabId
);
id_type!(
    /// A Herdr pane id, e.g. `w3:p12`.
    PaneId
);

impl PaneId {
    /// Numeric sort key so `w1:p2` orders before `w1:p10`.
    pub(crate) fn sort_key(&self) -> u64 {
        self.0
            .rsplit_once('p')
            .and_then(|(_, number)| number.parse().ok())
            .unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::PaneId;

    #[test]
    fn public_pane_ids_sort_numerically() {
        assert!(PaneId::from("w1:p2").sort_key() < PaneId::from("w1:p10").sort_key());
        assert_eq!(PaneId::from("unknown").sort_key(), u64::MAX);
    }
}
