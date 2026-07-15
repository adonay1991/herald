//! Agent adapters: one free function per agent protocol, each translating a
//! native payload into the canonical Event. Adapters exist only for stable,
//! well-known protocols; anyone else integrates through `herald emit --json`
//! (docs/CONTRACT.md). Returning Ok(None) means "not notification-worthy":
//! the hook exits 0 silently.

pub mod claude;
pub mod codex;
pub mod gemini;
