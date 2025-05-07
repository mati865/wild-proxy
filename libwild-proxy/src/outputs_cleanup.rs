use std::path::PathBuf;

pub(crate) struct DeleteOutputs {
    outputs: Vec<PathBuf>,
}

impl DeleteOutputs {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            outputs: Vec::with_capacity(capacity),
        }
    }

    pub(crate) fn add_output(&mut self, output: PathBuf) {
        self.outputs.push(output);
    }
}

impl Drop for DeleteOutputs {
    fn drop(&mut self) {
        let _span = tracing::info_span!("Delete outputs").entered();
        for output in &self.outputs {
            if let Err(e) = std::fs::remove_file(output) {
                tracing::warn!(
                    "Failed to delete output `{}`: {e}",
                    output.to_string_lossy()
                );
            }
        }
    }
}
