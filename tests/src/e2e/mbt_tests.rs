//! By default, these tests will run in release mode. This can be disabled
//! by setting environment variable `NAMADA_E2E_DEBUG=true`. For debugging,
//! you'll typically also want to set `RUST_BACKTRACE=1`, e.g.:
//!
//! ```ignore,shell
//! NAMADA_E2E_DEBUG=true RUST_BACKTRACE=1 cargo test e2e::mbt_tests -- --test-threads=1 --nocapture
//! ```
//!
//! To keep the temporary files created by a test, use env var
//! `NAMADA_E2E_KEEP_TEMP=true`.
#![allow(clippy::type_complexity)]

use std::str::FromStr;

use std::time::{Duration, Instant};

use color_eyre::eyre::Result;

use namada::types::storage::Epoch;

use namada_apps::config::genesis::genesis_config::{
    GenesisConfig, ParametersConfig, PosParamsConfig,
};

use crate::e2e::setup::constants::*;

use crate::e2e::helpers::{get_actor_rpc, get_epoch};
use crate::e2e::setup::NamadaBgCmd;
use crate::e2e::setup::{self, default_port_offset, Bin, Who};
use crate::{run, run_as};

use std::net::SocketAddr;

use namada::types::key::{self, ed25519, SigScheme};
use namada_apps::client;
use namada_apps::config::Config;

use crate::e2e::mbt::Reactor;

use std::collections::HashMap;

struct NamadaBlockchain {
    test: crate::e2e::setup::Test,
    validators: Vec<Option<NamadaBgCmd>>,
    wait_for_epoch: HashMap<String, Epoch>,
    accounts: HashMap<String, String>,
}

impl NamadaBlockchain {
    fn get_reactor() -> Result<Reactor<'static, Self>> {
        let mut mbt_reactor = Reactor::new("lastTx.tag", |state| {
            let num_of_validators: u8 = 1;

            let test = setup::network(
                |genesis| {
                    let parameters = ParametersConfig {
                        // probably fine, if not modified
                        min_num_of_blocks: 2,
                        // 5 secs per epoch
                        epochs_per_year: 60 * 60 * 24 * 365 / 5,
                        max_expected_time_per_block: 1,
                        ..genesis.parameters
                    };

                    let pos_params = PosParamsConfig {
                        pipeline_len: 1,
                        unbonding_len: 2,
                        ..genesis.pos_params
                    };

                    setup::set_validators(
                        num_of_validators,
                        GenesisConfig {
                            parameters,
                            pos_params,
                            ..genesis
                        },
                        default_port_offset,
                    )
                },
                None,
            )?;

            let validators = (0..(num_of_validators as u64))
                .map(|validator_id| {
                    let args = ["ledger"];
                    let mut validator = run_as!(
                        test,
                        Who::Validator(validator_id),
                        Bin::Node,
                        args,
                        Some(40)
                    )?;
                    validator.exp_string("Namada ledger node started")?;
                    validator.exp_string("This node is a validator")?;
                    Ok(Some(validator.background()))
                })
                .collect::<Result<Vec<_>>>()?;

            let accounts = [("val", "validator-0"), ("user2", BERTHA)]
                .into_iter()
                .map(|(x, y)| (x.into(), y.into()))
                .collect();

            Ok(Self {
                test,
                validators,
                wait_for_epoch: HashMap::default(),
                accounts,
            })
        });

        mbt_reactor.register("selfDelegate", |system, state| {
            let validator_one_rpc =
                get_actor_rpc(&system.test, &Who::Validator(0));

            let sender = state.get("lastTx.sender");
            let real_sender = system
                .accounts
                .get(sender.str())
                .map(|x| x.as_str())
                .expect("account is not present");
            assert_eq!(real_sender, "validator-0");
            let amount = state.get("lastTx.value").i64().to_string();

            let tx_args = vec![
                "bond",
                "--validator",
                real_sender,
                "--amount",
                &amount,
                "--gas-amount",
                "0",
                "--gas-limit",
                "0",
                "--gas-token",
                NAM,
                "--ledger-address",
                &validator_one_rpc,
            ];
            let mut client = run_as!(
                system.test,
                Who::Validator(0),
                Bin::Client,
                tx_args,
                Some(40)
            )?;
            client.exp_string("Transaction is valid.")?;
            client.assert_success();

            Ok(())
        });

        mbt_reactor.register("delegate", |system, state| {
            let validator_one_rpc =
                get_actor_rpc(&system.test, &Who::Validator(0));

            let sender = state.get("lastTx.sender");
            let real_sender = system
                .accounts
                .get(sender.str())
                .map(|x| x.as_str())
                .expect("account is not present");
            assert_eq!(real_sender, BERTHA);
            let amount = state.get("lastTx.value").i64().to_string();

            let tx_args = vec![
                "bond",
                "--validator",
                "validator-0",
                "--source",
                real_sender,
                "--amount",
                &amount,
                "--gas-amount",
                "0",
                "--gas-limit",
                "0",
                "--gas-token",
                NAM,
                "--ledger-address",
                &validator_one_rpc,
            ];
            let mut client = run!(system.test, Bin::Client, tx_args, Some(40))?;
            client.exp_string("Transaction is valid.")?;
            client.assert_success();

            Ok(())
        });

        mbt_reactor.register("selfUnbond", |system, state| {
            let validator_one_rpc =
                get_actor_rpc(&system.test, &Who::Validator(0));

            let sender = state.get("lastTx.sender");
            let real_sender = system
                .accounts
                .get(sender.str())
                .map(|x| x.as_str())
                .expect("account is not present");
            assert_eq!(real_sender, "validator-0");
            let amount = state.get("lastTx.value").i64().to_string();

            let tx_args = vec![
                "unbond",
                "--validator",
                real_sender,
                "--amount",
                &amount,
                "--gas-amount",
                "0",
                "--gas-limit",
                "0",
                "--gas-token",
                NAM,
                "--ledger-address",
                &validator_one_rpc,
            ];
            let mut client = run_as!(
                system.test,
                Who::Validator(0),
                Bin::Client,
                tx_args,
                Some(40)
            )?;
            let expected =
                format!("Amount {amount} withdrawable starting from epoch ");
            let (_unread, matched) =
                client.exp_regex(&format!("{expected}.*\n"))?;
            let epoch_raw = matched
                .trim()
                .split_once(&expected)
                .unwrap()
                .1
                .split_once('.')
                .unwrap()
                .0;
            let delegation_withdrawable_epoch =
                Epoch::from_str(epoch_raw).unwrap();

            println!("will wait till {delegation_withdrawable_epoch}");

            system.wait_for_epoch.insert(
                sender.str().to_string(),
                delegation_withdrawable_epoch,
            );

            client.assert_success();

            Ok(())
        });

        mbt_reactor.register("unbond", |system, state| {
            let validator_one_rpc =
                get_actor_rpc(&system.test, &Who::Validator(0));

            let sender = state.get("lastTx.sender");
            let real_sender = system
                .accounts
                .get(sender.str())
                .map(|x| x.as_str())
                .expect("account is not present");
            assert_eq!(real_sender, BERTHA);
            let amount = state.get("lastTx.value").i64().to_string();

            let tx_args = vec![
                "unbond",
                "--validator",
                "validator-0",
                "--source",
                real_sender,
                "--amount",
                &amount,
                "--gas-amount",
                "0",
                "--gas-limit",
                "0",
                "--gas-token",
                NAM,
                "--ledger-address",
                &validator_one_rpc,
            ];
            let mut client = run!(system.test, Bin::Client, tx_args, Some(40))?;
            let expected =
                format!("Amount {amount} withdrawable starting from epoch ");
            let (_unread, matched) =
                client.exp_regex(&format!("{expected}.*\n"))?;
            let epoch_raw = matched
                .trim()
                .split_once(&expected)
                .unwrap()
                .1
                .split_once('.')
                .unwrap()
                .0;
            let delegation_withdrawable_epoch =
                Epoch::from_str(epoch_raw).unwrap();

            println!("will wait till {delegation_withdrawable_epoch}");

            system.wait_for_epoch.insert(
                sender.str().to_string(),
                delegation_withdrawable_epoch,
            );

            client.assert_success();

            Ok(())
        });

        mbt_reactor.register("selfWithdraw", |system, state| {
            let validator_one_rpc =
                get_actor_rpc(&system.test, &Who::Validator(0));

            let sender = state.get("lastTx.sender");
            let real_sender = system
                .accounts
                .get(sender.str())
                .map(|x| x.as_str())
                .expect("account is not present");
            assert_eq!(real_sender, "validator-0");

            let tx_args = vec![
                "withdraw",
                "--validator",
                real_sender,
                "--gas-amount",
                "0",
                "--gas-limit",
                "0",
                "--gas-token",
                NAM,
                "--ledger-address",
                &validator_one_rpc,
            ];
            let mut client = run_as!(
                system.test,
                Who::Validator(0),
                Bin::Client,
                tx_args,
                Some(40)
            )?;
            client.exp_string("Transaction is valid.")?;
            client.assert_success();

            Ok(())
        });

        mbt_reactor.register("withdraw", |system, state| {
            let validator_one_rpc =
                get_actor_rpc(&system.test, &Who::Validator(0));

            let sender = state.get("lastTx.sender");
            let real_sender = system
                .accounts
                .get(sender.str())
                .map(|x| x.as_str())
                .expect("account is not present");
            assert_eq!(real_sender, BERTHA);

            // Submit a withdrawal of the delegation
            let tx_args = vec![
                "withdraw",
                "--validator",
                "validator-0",
                "--source",
                real_sender,
                "--gas-amount",
                "0",
                "--gas-limit",
                "0",
                "--gas-token",
                NAM,
                "--ledger-address",
                &validator_one_rpc,
            ];
            let mut client = run!(system.test, Bin::Client, tx_args, Some(40))?;
            client.exp_string("Transaction is valid.")?;
            client.assert_success();

            Ok(())
        });

        mbt_reactor.register("waitForEpoch", |system, state| {
            let validator_one_rpc =
                get_actor_rpc(&system.test, &Who::Validator(0));
            let epoch = get_epoch(&system.test, &validator_one_rpc)?;

            let sender = state.get("lastTx.sender");

            let delegation_withdrawable_epoch = system
                .wait_for_epoch
                .remove(sender.str())
                .expect("no future epoch to wait for account");

            println!(
                "Current epoch: {}, earliest epoch for withdrawal: {}",
                epoch, delegation_withdrawable_epoch
            );
            let start = Instant::now();
            let loop_timeout = Duration::new(40, 0);
            loop {
                if Instant::now().duration_since(start) > loop_timeout {
                    panic!(
                        "Timed out waiting for epoch: {}",
                        delegation_withdrawable_epoch
                    );
                }
                let epoch = get_epoch(&system.test, &validator_one_rpc)?;
                if epoch >= delegation_withdrawable_epoch {
                    break;
                }
            }

            Ok(())
        });

        mbt_reactor.register("endOfEpoch", |system, _state| {
            let validator_one_rpc =
                get_actor_rpc(&system.test, &Who::Validator(0));
            let epoch = get_epoch(&system.test, &validator_one_rpc)?;

            let delegation_withdrawable_epoch = Epoch(epoch.0 + 1);

            println!(
                "Current epoch: {}, earliest epoch for withdrawal: {}",
                epoch, delegation_withdrawable_epoch
            );
            let start = Instant::now();
            let loop_timeout = Duration::new(40, 0);
            loop {
                if Instant::now().duration_since(start) > loop_timeout {
                    panic!(
                        "Timed out waiting for epoch: {}",
                        delegation_withdrawable_epoch
                    );
                }
                let epoch = get_epoch(&system.test, &validator_one_rpc)?;
                if epoch >= delegation_withdrawable_epoch {
                    break;
                }
            }

            Ok(())
        });

        mbt_reactor.register("evidence", |system, _state| {
            // Copy the first genesis validator base-dir
            let validator_0_base_dir =
                system.test.get_base_dir(&Who::Validator(0));
            let validator_0_base_dir_copy =
                system.test.test_dir.path().join("validator-0-copy");
            fs_extra::dir::copy(
                validator_0_base_dir,
                &validator_0_base_dir_copy,
                &fs_extra::dir::CopyOptions {
                    copy_inside: true,
                    ..Default::default()
                },
            )
            .unwrap();

            // Increment its ports and generate new node ID to avoid conflict

            // Same as in `genesis/e2e-tests-single-node.toml` for `validator-0`
            let net_address_0 =
                SocketAddr::from_str("127.0.0.1:27656").unwrap();
            let net_address_port_0 = net_address_0.port();

            let update_config = |ix: u8, mut config: Config| {
                let first_port = net_address_port_0 + 6 * (ix as u16 + 1);
                config.ledger.tendermint.p2p_address.set_port(first_port);
                config
                    .ledger
                    .tendermint
                    .rpc_address
                    .set_port(first_port + 1);
                config.ledger.shell.ledger_address.set_port(first_port + 2);
                config
            };

            let validator_0_copy_config = update_config(
                2,
                Config::load(
                    &validator_0_base_dir_copy,
                    &system.test.net.chain_id,
                    None,
                ),
            );
            validator_0_copy_config
                .write(
                    &validator_0_base_dir_copy,
                    &system.test.net.chain_id,
                    true,
                )
                .unwrap();

            // Generate a new node key
            use rand::prelude::ThreadRng;
            use rand::thread_rng;

            let mut rng: ThreadRng = thread_rng();
            let node_sk = ed25519::SigScheme::generate(&mut rng);
            let node_sk = key::common::SecretKey::Ed25519(node_sk);
            let tm_home_dir = validator_0_base_dir_copy
                .join(system.test.net.chain_id.as_str())
                .join("tendermint");
            let _node_pk =
                client::utils::write_tendermint_node_key(&tm_home_dir, node_sk);

            let args = ["ledger"];

            // Run it to get it to double vote and sign block
            let loc = format!("{}:{}", std::file!(), std::line!());
            // This node will only connect to `validator_1`, so that nodes
            // `validator_0` and `validator_0_copy` should start double signing
            let mut validator_0_copy = setup::run_cmd(
                Bin::Node,
                args,
                Some(40),
                &system.test.working_dir,
                validator_0_base_dir_copy,
                "validator",
                loc,
            )?;
            validator_0_copy.exp_string("Namada ledger node started")?;
            validator_0_copy.exp_string("This node is a validator")?;
            let _bg_validator_0_copy = validator_0_copy.background();

            println!("clone validator started");

            // Submit a valid token transfer tx to validator 0
            let validator_one_rpc =
                get_actor_rpc(&system.test, &Who::Validator(0));
            let tx_args = [
                "transfer",
                "--source",
                BERTHA,
                "--target",
                ALBERT,
                "--token",
                NAM,
                "--amount",
                "10.1",
                "--gas-amount",
                "0",
                "--gas-limit",
                "0",
                "--gas-token",
                NAM,
                "--ledger-address",
                &validator_one_rpc,
            ];
            let mut client = run!(system.test, Bin::Client, tx_args, Some(40))?;
            client.exp_string("Transaction is valid.")?;
            client.assert_success();

            // Wait for double signing evidence
            let mut validator_1 = system.validators[1]
                .take()
                .expect("validator background command is not present")
                .foreground();
            validator_1.exp_string("Processing evidence")?;
            validator_1.exp_string("Slashing")?;

            println!("validator slashed");

            system.validators[1] = Some(validator_1.background());

            Ok(())
        });

        mbt_reactor.register_invariant_state(|system, state| {
            let validator_one_rpc =
                get_actor_rpc(&system.test, &Who::Validator(0));
            let balance_offset = system
                .accounts
                .iter()
                .map(|(tla_acc, blk_acc)| {
                    let tx_args = [
                        "balance",
                        "--owner",
                        blk_acc,
                        "--token",
                        NAM,
                        "--ledger-address",
                        &validator_one_rpc,
                    ];
                    let mut client =
                        run!(system.test, Bin::Client, tx_args, Some(40))?;
                    let (_unread, matched) =
                        client.exp_regex(&format!("{NAM}:\\s+\\d+\r?\n"))?;
                    let blk_balance: i64 =
                        matched.trim().rsplit_once(" ").unwrap().1.parse()?;
                    client.assert_success();

                    let tla_balance = state
                        .get(&format!("balanceOf.\\#map.#(0=\"{tla_acc}\").1"))
                        .i64();

                    assert!(blk_balance > tla_balance);

                    Ok((tla_acc, blk_balance - tla_balance))
                })
                .collect::<Result<HashMap<_, _>>>()?;

            std::thread::sleep(std::time::Duration::from_secs(2));

            Ok(serde_json::to_value(&balance_offset)?)
        });

        Ok(mbt_reactor)
    }
}

#[test_case::test_case("src/e2e/data/traces/example1.itf.json")]
fn mbt_pos(path: &str) -> Result<()> {
    let json_string = std::fs::read_to_string(path)?;
    let json_value = gjson::parse(&json_string);
    NamadaBlockchain::get_reactor()?.test(&json_value.get("states").array())?;
    Ok(())
}