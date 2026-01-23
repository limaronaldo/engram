//! Permission system for access control

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Permission types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Read access
    Read,
    /// Write/create access
    Write,
    /// Update existing resources
    Update,
    /// Delete access
    Delete,
    /// Share with others
    Share,
    /// Administrative access
    Admin,
}

/// Resource types that can be protected
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    /// Memory records
    Memory,
    /// Cross-references
    CrossRef,
    /// Tags
    Tag,
    /// Namespaces
    Namespace,
    /// Users
    User,
    /// API keys
    ApiKey,
    /// System-level operations
    System,
}

/// A set of permissions for various resources
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionSet {
    /// Set of (permission, resource) tuples
    permissions: HashSet<(Permission, ResourceType)>,
    /// Whether this is an admin set (all permissions)
    is_admin: bool,
}

impl PermissionSet {
    /// Create an empty permission set
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a permission set from a list of permissions
    pub fn from_permissions(perms: Vec<(Permission, ResourceType)>) -> Self {
        Self {
            permissions: perms.into_iter().collect(),
            is_admin: false,
        }
    }

    /// Create an admin permission set (all permissions)
    pub fn admin() -> Self {
        Self {
            permissions: HashSet::new(),
            is_admin: true,
        }
    }

    /// Create a read-only permission set
    pub fn read_only() -> Self {
        let mut permissions = HashSet::new();
        permissions.insert((Permission::Read, ResourceType::Memory));
        permissions.insert((Permission::Read, ResourceType::CrossRef));
        permissions.insert((Permission::Read, ResourceType::Tag));
        Self {
            permissions,
            is_admin: false,
        }
    }

    /// Create a standard user permission set
    pub fn standard_user() -> Self {
        let resources = [
            ResourceType::Memory,
            ResourceType::CrossRef,
            ResourceType::Tag,
        ];
        let permissions = [
            Permission::Read,
            Permission::Write,
            Permission::Update,
            Permission::Delete,
        ];

        let mut set = HashSet::new();
        for resource in resources {
            for permission in permissions {
                set.insert((permission, resource));
            }
        }

        // Users can read their own API keys
        set.insert((Permission::Read, ResourceType::ApiKey));
        set.insert((Permission::Write, ResourceType::ApiKey));
        set.insert((Permission::Delete, ResourceType::ApiKey));

        Self {
            permissions: set,
            is_admin: false,
        }
    }

    /// Add a permission
    pub fn add(&mut self, permission: Permission, resource: ResourceType) {
        self.permissions.insert((permission, resource));
    }

    /// Remove a permission
    pub fn remove(&mut self, permission: Permission, resource: ResourceType) {
        self.permissions.remove(&(permission, resource));
    }

    /// Check if a permission exists
    pub fn has_permission(&self, permission: Permission, resource: ResourceType) -> bool {
        if self.is_admin {
            return true;
        }
        self.permissions.contains(&(permission, resource))
    }

    /// Check if this is an admin set
    pub fn is_admin(&self) -> bool {
        self.is_admin
    }

    /// Merge another permission set into this one
    pub fn merge(&mut self, other: &PermissionSet) {
        if other.is_admin {
            self.is_admin = true;
        }
        self.permissions.extend(other.permissions.iter().cloned());
    }

    /// Get all permissions as a vector
    pub fn to_vec(&self) -> Vec<(Permission, ResourceType)> {
        self.permissions.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_set_basic() {
        let mut set = PermissionSet::new();
        set.add(Permission::Read, ResourceType::Memory);

        assert!(set.has_permission(Permission::Read, ResourceType::Memory));
        assert!(!set.has_permission(Permission::Write, ResourceType::Memory));
        assert!(!set.has_permission(Permission::Read, ResourceType::User));
    }

    #[test]
    fn test_admin_set() {
        let set = PermissionSet::admin();
        assert!(set.has_permission(Permission::Admin, ResourceType::System));
        assert!(set.has_permission(Permission::Delete, ResourceType::User));
        assert!(set.has_permission(Permission::Read, ResourceType::Memory));
    }

    #[test]
    fn test_standard_user() {
        let set = PermissionSet::standard_user();
        assert!(set.has_permission(Permission::Read, ResourceType::Memory));
        assert!(set.has_permission(Permission::Write, ResourceType::Memory));
        assert!(set.has_permission(Permission::Delete, ResourceType::Memory));
        assert!(!set.has_permission(Permission::Admin, ResourceType::System));
        assert!(!set.has_permission(Permission::Delete, ResourceType::User));
    }

    #[test]
    fn test_merge() {
        let mut set1 = PermissionSet::new();
        set1.add(Permission::Read, ResourceType::Memory);

        let mut set2 = PermissionSet::new();
        set2.add(Permission::Write, ResourceType::Memory);

        set1.merge(&set2);

        assert!(set1.has_permission(Permission::Read, ResourceType::Memory));
        assert!(set1.has_permission(Permission::Write, ResourceType::Memory));
    }

    #[test]
    fn test_serialization() {
        let set = PermissionSet::standard_user();
        let json = serde_json::to_string(&set).unwrap();
        let restored: PermissionSet = serde_json::from_str(&json).unwrap();

        assert!(restored.has_permission(Permission::Read, ResourceType::Memory));
        assert!(restored.has_permission(Permission::Write, ResourceType::Memory));
    }
}
