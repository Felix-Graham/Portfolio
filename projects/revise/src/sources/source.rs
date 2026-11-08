/// A registered external tool (direct request: "an API... other
/// revision applications... to be registered and accounted for") —
/// e.g. a person's own French Quiz script. Created once via
/// `revise-cli register <name>` (or from "Manage Sources" in the app),
/// then referenced by every `ExternalContribution` it logs afterward.
/// Deliberately minimal: no token or secret (see `crate::sources` for
/// why) — just a name, unique so `--source <name>` on the CLI never
/// resolves ambiguously.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub id: i64,
    pub name: String,
    pub created_at: String,
}
