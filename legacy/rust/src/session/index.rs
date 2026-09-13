#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeSelector {
    Latest,
    Exact(String),
    Prefix(String),
}
