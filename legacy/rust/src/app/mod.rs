use crate::runtime::Runtime;

pub struct App {
    runtime: Runtime,
}

impl App {
    pub fn new() -> Self {
        Self {
            runtime: Runtime::new(),
        }
    }

    pub fn run(&self, args: &[String]) -> Result<i32, String> {
        self.runtime.run(args)
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
