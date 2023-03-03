use color_eyre::eyre::Result;
use eyre::eyre;

type ReactorFunc<S> = fn(&mut S, &str, &gjson::Value) -> Result<()>;

#[derive(Default)]
pub struct Reactor<'a, S> {
    reactors: std::collections::HashMap<
        &'a str,
        ReactorFunc<S>,
    >,
    sequence_reactors: std::collections::HashMap<&'a str, Vec<&'a str>>,
}

impl<'a, S> Reactor<'a, S> {
    pub fn register<'b>(
        &mut self,
        tag: &'b str,
        func: ReactorFunc<S>,
    ) where
        'b: 'a,
    {
        self.reactors.insert(tag, func);
    }

    pub fn register_sequence<'b>(&mut self, tag: &'b str, tags: Vec<&'b str>)
    where
        'b: 'a,
    {
        for t in &tags {
            assert!(self.reactors.contains_key(t))
        }

        self.sequence_reactors.insert(tag, tags);
    }

    fn execute(
        &self,
        system: &mut S,
        tag: &str,
        state: &gjson::Value,
    ) -> Result<()> {
        if let Some(f) = self.reactors.get(tag) {
            f(system, tag, state)
        } else if let Some(tags) = self.sequence_reactors.get(tag) {
            for t in tags {
                self.execute(system, t, state)?
            }
            Ok(())
        } else {
            Err(eyre!(format!("tag: {} is not registered.", tag)))
        }
    }

    pub fn run_with_system<F>(
        &self,
        system_f: F,
        states: &[gjson::Value],
    ) -> Result<()>
    where
        F: FnOnce() -> S,
    {
        let mut system = system_f();
        for e_state in states {
            let tag = e_state.get("lastTx.tag");
            self.execute(&mut system, tag.str(), e_state)?;
        }
        Ok(())
    }

    pub fn run_with_default_system(&self, states: &[gjson::Value]) -> Result<()>
    where
        S: Default,
    {
        self.run_with_system(Default::default, states)
    }
}
