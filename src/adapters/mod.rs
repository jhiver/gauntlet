//! Harness adapters. Registry maps adapter module names to classes.

pub mod agy;
pub mod base;
pub mod cmd;
pub mod codex;
pub mod echo;
pub mod human;
pub mod kimi;
pub mod reasonix;

pub use agy::AgyAdapter;
pub use base::{
    AdapterConfig, ErrorPatternsConfig, FailureKind, HarnessAdapter, RunResult, SubprocessAdapter,
    STDERR_TAIL,
};
pub use cmd::CmdAdapter;
pub use codex::CodexAdapter;
pub use echo::EchoAdapter;
pub use human::{read_decision, HumanAdapter};
pub use kimi::KimiAdapter;
pub use reasonix::ReasonixAdapter;

pub fn create_adapter(
    adapter_name: &str,
    harness_name: &str,
    cfg: Option<&AdapterConfig>,
) -> Result<Box<dyn HarnessAdapter>, String> {
    match adapter_name {
        "agy" => Ok(Box::new(AgyAdapter::new(harness_name, cfg))),
        "cmd" => Ok(Box::new(CmdAdapter::new(harness_name, cfg))),
        "codex" => Ok(Box::new(CodexAdapter::new(harness_name, cfg))),
        "echo" => Ok(Box::new(EchoAdapter::new(harness_name, cfg))),
        "human" => Ok(Box::new(HumanAdapter::new(harness_name, cfg))),
        "kimi" => Ok(Box::new(KimiAdapter::new(harness_name, cfg))),
        "reasonix" => Ok(Box::new(ReasonixAdapter::new(harness_name, cfg))),
        other => Err(format!("unknown adapter '{}'", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_all_adapters() {
        let names = ["agy", "cmd", "codex", "echo", "human", "kimi", "reasonix"];
        for name in names {
            let adapter = create_adapter(name, name, None);
            assert!(adapter.is_ok(), "failed to create adapter {}", name);
            assert_eq!(adapter.unwrap().name(), name);
        }
        assert!(create_adapter("unknown", "unknown", None).is_err());
    }
}
