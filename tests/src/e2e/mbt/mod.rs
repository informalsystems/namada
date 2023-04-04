//! A simple library for model-based testing in Rust.
//!
//! This library provides a `Reactor` struct that allows users to define a
//! model, as well as step and invariant functions for that model,
//! and then execute a test run based on a given set of input states.
//!
//! # Examples
//!
//! This example demonstrates how to use the MBT library for a simple banking system.
//!
//! ```rust
//! use color_eyre::eyre::{eyre, Result};
//! use gjson::{self, Value as GJsonValue};
//! use serde_json::{json, Value as SerdeJsonValue};
//! use mbt::Reactor;
//!
//! struct Bank {
//!     accounts: std::collections::HashMap<String, f64>,
//! }
//!
//! fn init_bank(state: &GJsonValue) -> Result<Bank> {
//!     let mut bank = Bank {
//!         accounts: Default::default(),
//!     };
//!     state.get("accounts").each(|name, balance| {
//!         bank.accounts.insert(name.to_string(), balance.f64());
//!         true
//!     });
//!     Ok(bank)
//! }
//!
//! fn deposit(bank: &mut Bank, state: &GJsonValue) -> Result<()> {
//!     let account = state.get("account").to_string();
//!     let amount = state.get("amount").f64();
//!     bank.accounts.entry(account).and_modify(|b| *b += amount);
//!     Ok(())
//! }
//!
//! fn withdraw(bank: &mut Bank, state: &GJsonValue) -> Result<()> {
//!     let account = state.get("account").to_string();
//!     let amount = state.get("amount").f64();
//!     let balance = bank
//!         .accounts
//!         .get_mut(&account)
//!         .ok_or_else(|| eyre!("Account not found"))?;
//!     if *balance >= amount {
//!         *balance -= amount;
//!         Ok(())
//!     } else {
//!         Err(eyre!("Insufficient balance"))
//!     }
//! }
//!
//! fn transfer(bank: &mut Bank, state: &GJsonValue) -> Result<()> {
//!     let src_account = state.get("src_account").to_string();
//!     let dst_account = state.get("dst_account").to_string();
//!     let amount = state.get("amount").f64();
//!
//!     let src_balance = bank
//!         .accounts
//!         .get_mut(&src_account)
//!         .ok_or_else(|| eyre!("Source account not found"))?;
//!
//!     if *src_balance >= amount {
//!         *src_balance -= amount;
//!         let dst_balance = bank
//!             .accounts
//!             .get_mut(&dst_account)
//!             .ok_or_else(|| eyre!("Destination account not found"))?;
//!         *dst_balance += amount;
//!         Ok(())
//!     } else {
//!         Err(eyre!("Insufficient balance in source account"))
//!     }
//! }
//!
//! fn positive_balance(bank: &mut Bank, _state: &GJsonValue) -> Result<bool> {
//!     Ok(bank.accounts.values().all(|&balance| balance >= 0.0))
//! }
//!
//! fn total_supply(
//!     bank: &mut Bank,
//!     _state: &GJsonValue,
//! ) -> Result<SerdeJsonValue> {
//!     let total: f64 = bank.accounts.values().cloned().sum();
//!     Ok(json!({ "total_supply": total }))
//! }
//!
//! #[test]
//! fn test_bank() -> Result<()> {
//!     let actions = vec![
//!         gjson::parse(
//!             r#"{ "tag": "init", "accounts": { "Alice": 1000.0, "Bob": 500.0 }}"#,
//!         ),
//!         gjson::parse(
//!             r#"{ "tag": "transfer", "src_account": "Alice", "dst_account": "Bob", "amount": 200.0 }"#,
//!         ),
//!         gjson::parse(
//!             r#"{ "tag": "transfer", "src_account": "Bob", "dst_account": "Alice", "amount": 150.0 }"#,
//!         ),
//!         gjson::parse(
//!             r#"{ "tag": "withdraw", "account": "Bob", "amount": 50.0 }"#,
//!         ),
//!         gjson::parse(
//!             r#"{ "tag": "deposit", "account": "Alice", "amount": 100.0 }"#,
//!         ),
//!     ];
//!
//!     let mut reactor = Reactor::new("tag", init_bank);
//!     reactor.register("deposit", deposit);
//!     reactor.register("withdraw", withdraw);
//!     reactor.register("transfer", transfer);
//!     reactor.register_invariant(positive_balance);
//!     reactor.register_invariant_state(total_supply);
//!     reactor.test(&actions)?;
//!
//!     Ok(())
//! }
//! ```

use color_eyre::eyre::Result;
use eyre::eyre;

type InitReactor<S> = fn(&gjson::Value) -> Result<S>;
type StepReactor<S> = fn(&mut S, &gjson::Value) -> Result<()>;
type InvReactor<S> = fn(&mut S, &gjson::Value) -> Result<bool>;
type InvStateReactor<S> =
    fn(&mut S, &gjson::Value) -> Result<serde_json::Value>;

use color_eyre::owo_colors::OwoColorize;

use std::time::SystemTime;

pub struct Reactor<'a, S> {
    tag_path: &'a str,
    init_reactor: InitReactor<S>,
    step_reactors: std::collections::HashMap<&'a str, StepReactor<S>>,
    inv_reactors: Vec<InvReactor<S>>,
    inv_state_reactors: Vec<InvStateReactor<S>>,
    sequence_reactors: std::collections::HashMap<&'a str, Vec<&'a str>>,
}

impl<'a, S> Reactor<'a, S> {
    pub fn new(tag_path: &'a str, init_reactor: InitReactor<S>) -> Self {
        Self {
            tag_path,
            init_reactor,
            step_reactors: Default::default(),
            inv_reactors: Default::default(),
            inv_state_reactors: Default::default(),
            sequence_reactors: Default::default(),
        }
    }
}

impl<'a, S> Reactor<'a, S> {
    /// Registers a step function `func` for the given `tag`.
    pub fn register<'b>(&mut self, tag: &'b str, func: StepReactor<S>)
    where
        'b: 'a,
    {
        self.step_reactors.insert(tag, func);
    }

    /// Registers a sequence of step functions for the given `tag`.
    pub fn register_sequence<'b>(&mut self, tag: &'b str, tags: Vec<&'b str>)
    where
        'b: 'a,
    {
        for t in &tags {
            assert!(self.step_reactors.contains_key(t))
        }

        self.sequence_reactors.insert(tag, tags);
    }

    /// Registers an invariant function `func`.
    pub fn register_invariant<'b>(&mut self, func: InvReactor<S>)
    where
        'b: 'a,
    {
        self.inv_reactors.push(func);
    }

    /// Registers an invariant state function `func`.
    pub fn register_invariant_state<'b>(&mut self, func: InvStateReactor<S>)
    where
        'b: 'a,
    {
        self.inv_state_reactors.push(func);
    }

    fn execute(
        &self,
        system: &mut S,
        tag: &str,
        state: &gjson::Value,
    ) -> Result<()> {
        if let Some(f) = self.step_reactors.get(tag) {
            f(system, state)
        } else if let Some(tags) = self.sequence_reactors.get(tag) {
            for t in tags {
                self.execute(system, t, state)?
            }
            Ok(())
        } else {
            Err(eyre!(format!("tag: {} is not registered.", tag)))
        }
    }

    /// Tests the given sequence of `states` using the registered step, sequence,
    /// and invariant functions.
    pub fn test(&self, states: &[gjson::Value]) -> Result<()> {
        let mut inv_states = vec![];
        let time = SystemTime::now();

        fn mbt_log(
            time: SystemTime,
            index: usize,
            tag: &str,
            data: &str,
        ) -> Result<()> {
            println!(
                "[{} {: >4}s] {: >4}:{: <10}> {}",
                "MBT".bright_blue(),
                time.elapsed()?.as_secs().green(),
                index,
                tag.yellow(),
                data
            );
            Ok(())
        }

        let mut system = states
            .first()
            .ok_or_else(|| eyre!("trace is empty"))
            .and_then(|f_state| {
                let mut system = (self.init_reactor)(&f_state)?;
                for inv in self.inv_reactors.iter() {
                    assert!(inv(&mut system, f_state)?);
                }
                for inv_st in self.inv_state_reactors.iter() {
                    inv_states.push(inv_st(&mut system, f_state)?);
                }

                Ok(system)
            })?;
        for (i_state, e_state) in states.iter().enumerate().skip(1) {
            let tag = e_state.get(self.tag_path);
            mbt_log(time, i_state, tag.str(), "Executing Step")?;
            self.execute(&mut system, tag.str(), e_state)?;
            for inv in self.inv_reactors.iter() {
                mbt_log(time, i_state, tag.str(), "Executing Inv")?;
                inv(&mut system, e_state)?;
            }

            for (inv_st, st) in
                self.inv_state_reactors.iter().zip(inv_states.iter())
            {
                mbt_log(time, i_state, tag.str(), "Executing Inv Step")?;
                assert_eq!(st, &inv_st(&mut system, e_state)?);
            }
        }
        Ok(())
    }
}
