use serde::{Deserialize, Serialize};
use specta::Type;

macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
        #[serde(transparent)]
        pub struct $name(pub i64);

        impl From<i64> for $name {
            fn from(v: i64) -> Self { Self(v) }
        }
        impl From<$name> for i64 {
            fn from(v: $name) -> Self { v.0 }
        }
    };
}

id_newtype!(FolderId);
id_newtype!(ObjectId);
id_newtype!(AssetId);
id_newtype!(TagId);
