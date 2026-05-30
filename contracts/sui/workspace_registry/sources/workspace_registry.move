/// RECALL on-chain workspace ownership and capability registry.
/// Each workspace has an owner (the operator). Capabilities are registered on-chain
/// so that any party can verify their validity without trusting the control plane.
#[allow(lint(public_entry))]
module recall_workspace_registry::workspace_registry {
    use sui::event;
    use std::string::{Self, String};
    use sui::clock::{Self, Clock};
    use sui::table::{Self, Table};

    /// On-chain workspace record. Owned by the operator.
    public struct WorkspaceRecord has key, store {
        id: UID,
        workspace_id: String,
        name: String,
        /// Topology: "CLOSED" or "OPEN"
        topology_mode: String,
        active_constitution_version: String,
        created_at_ms: u64,
        /// Walrus blob ID of the latest workspace snapshot.
        snapshot_blob_id: String,
    }

    /// Global workspace registry — shared object, one per deployment.
    public struct GlobalRegistry has key {
        id: UID,
        /// workspace_id -> WorkspaceRecord ID
        workspace_ids: Table<String, ID>,
        total_workspaces: u64,
    }

    /// Capability token issued to an agent. Owned by the capability holder.
    public struct CapabilityToken has key, store {
        id: UID,
        capability_id: String,
        workspace_id: String,
        holder_passport_id: String,
        /// Comma-separated action kinds
        permitted_actions: String,
        valid_until_ms: u64,
        issued_at_ms: u64,
        attenuation_depth: u8,
    }

    public struct WorkspaceCreated has copy, drop {
        workspace_id: String,
        name: String,
        topology_mode: String,
        created_at_ms: u64,
    }

    public struct CapabilityIssued has copy, drop {
        capability_id: String,
        workspace_id: String,
        holder_passport_id: String,
        valid_until_ms: u64,
    }

    fun init(ctx: &mut TxContext) {
        let registry = GlobalRegistry {
            id: object::new(ctx),
            workspace_ids: table::new(ctx),
            total_workspaces: 0,
        };
        transfer::share_object(registry);
    }

    /// Create a new workspace on-chain. The caller becomes the operator.
    public entry fun create_workspace(
        registry: &mut GlobalRegistry,
        workspace_id: vector<u8>,
        name: vector<u8>,
        topology_mode: vector<u8>,
        constitution_version: vector<u8>,
        clock: &Clock,
        ctx: &mut TxContext,
    ) {
        let ws_id_str    = string::utf8(workspace_id);
        let name_str     = string::utf8(name);
        let topology_str = string::utf8(topology_mode);
        let created_at   = clock::timestamp_ms(clock);

        let record = WorkspaceRecord {
            id: object::new(ctx),
            workspace_id: ws_id_str,
            name: name_str,
            topology_mode: topology_str,
            active_constitution_version: string::utf8(constitution_version),
            created_at_ms: created_at,
            snapshot_blob_id: string::utf8(b""),
        };

        let record_id = object::id(&record);
        table::add(&mut registry.workspace_ids, ws_id_str, record_id);
        registry.total_workspaces = registry.total_workspaces + 1;

        event::emit(WorkspaceCreated {
            workspace_id: ws_id_str,
            name: name_str,
            topology_mode: topology_str,
            created_at_ms: created_at,
        });

        transfer::transfer(record, tx_context::sender(ctx));
    }

    /// Issue a capability token on-chain. The holder receives a CapabilityToken object.
    public entry fun issue_capability(
        workspace_id: vector<u8>,
        capability_id: vector<u8>,
        holder_address: address,
        holder_passport_id: vector<u8>,
        permitted_actions: vector<u8>,
        valid_until_ms: u64,
        clock: &Clock,
        ctx: &mut TxContext,
    ) {
        let cap_id_str  = string::utf8(capability_id);
        let ws_str      = string::utf8(workspace_id);
        let holder_str  = string::utf8(holder_passport_id);
        let issued_at   = clock::timestamp_ms(clock);

        event::emit(CapabilityIssued {
            capability_id: cap_id_str,
            workspace_id: ws_str,
            holder_passport_id: holder_str,
            valid_until_ms,
        });

        let token = CapabilityToken {
            id: object::new(ctx),
            capability_id: cap_id_str,
            workspace_id: ws_str,
            holder_passport_id: holder_str,
            permitted_actions: string::utf8(permitted_actions),
            valid_until_ms,
            issued_at_ms: issued_at,
            attenuation_depth: 0,
        };

        transfer::transfer(token, holder_address);
    }

    /// Update the workspace snapshot blob ID after a snapshot is taken.
    public entry fun update_snapshot(
        record: &mut WorkspaceRecord,
        new_blob_id: vector<u8>,
        _ctx: &mut TxContext,
    ) {
        record.snapshot_blob_id = string::utf8(new_blob_id);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Accessors
    // ─────────────────────────────────────────────────────────────────────────

    public fun workspace_id(record: &WorkspaceRecord): &String          { &record.workspace_id }
    public fun name(record: &WorkspaceRecord): &String                  { &record.name }
    public fun topology_mode(record: &WorkspaceRecord): &String         { &record.topology_mode }
    public fun snapshot_blob_id(record: &WorkspaceRecord): &String      { &record.snapshot_blob_id }
    public fun created_at_ms(record: &WorkspaceRecord): u64             { record.created_at_ms }
    public fun total_workspaces(registry: &GlobalRegistry): u64         { registry.total_workspaces }
    public fun capability_id(token: &CapabilityToken): &String          { &token.capability_id }
    public fun cap_workspace_id(token: &CapabilityToken): &String       { &token.workspace_id }
    public fun valid_until_ms(token: &CapabilityToken): u64             { token.valid_until_ms }
    public fun attenuation_depth(token: &CapabilityToken): u8           { token.attenuation_depth }
    public fun has_workspace(registry: &GlobalRegistry, ws_id: String): bool {
        table::contains(&registry.workspace_ids, ws_id)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test-only helpers
    // ─────────────────────────────────────────────────────────────────────────

    #[test_only]
    public fun init_for_testing(ctx: &mut TxContext) {
        init(ctx);
    }
}
