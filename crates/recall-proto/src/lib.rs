// Re-export all generated protobuf/gRPC types.
// The inner `recall::*::v1` hierarchy mirrors proto package names so generated
// super:: paths (e.g. super::super::common::v1::Hash) resolve correctly.
// Flat re-exports at the module level allow the common import style used across crates.

pub mod recall {
    pub mod common {
        pub mod v1 {
            tonic::include_proto!("recall.common.v1");
        }
    }
    pub mod passport {
        pub mod v1 {
            tonic::include_proto!("recall.passport.v1");
        }
    }
    pub mod receipt {
        pub mod v1 {
            tonic::include_proto!("recall.receipt.v1");
        }
    }
    pub mod capability {
        pub mod v1 {
            tonic::include_proto!("recall.capability.v1");
        }
    }
    pub mod memory {
        pub mod v1 {
            tonic::include_proto!("recall.memory.v1");
        }
    }
    pub mod registry {
        pub mod v1 {
            tonic::include_proto!("recall.registry.v1");
        }
    }
    pub mod controlplane {
        pub mod v1 {
            tonic::include_proto!("recall.controlplane.v1");
        }
    }
}

// Flat aliases: recall_proto::common::TypeName, recall_proto::receipt::TypeName, etc.
pub use recall::common::v1 as common;
pub use recall::passport::v1 as passport;
pub use recall::receipt::v1 as receipt;
pub use recall::capability::v1 as capability;
pub use recall::memory::v1 as memory;
pub use recall::registry::v1 as registry;

// controlplane exposes both flat (recall_proto::controlplane::Type) and nested
// (recall_proto::controlplane::v1::Type) so either import style works.
pub mod controlplane {
    pub mod v1 {
        pub use crate::recall::controlplane::v1::*;
    }
    pub use self::v1::*;
}
