#![allow(missing_docs)]

use revm_interpreter::SharedMemory;
use revm_primitives::{b256, Env, U256};
use revmc_cli::Bench;

#[test]
fn test_all_benches() {
    let benches = revmc_cli::get_benches();
    for Bench { name, bytecode, calldata, .. } in benches {
        let gas_limit = 1_000_000_000;
        println!("Testing: {}", name);
        let mut env = Env::default();
        env.tx.gas_limit = gas_limit;
        env.tx.data = calldata.clone().into();
        let bytecode = revm_interpreter::analysis::to_analysed(revm_primitives::Bytecode::new_raw(
            revm_primitives::Bytes::copy_from_slice(&bytecode),
        ));
        let contract = revm_interpreter::Contract::new_env(&env, bytecode, None);

        // 从 bench.rs 中复制的存储初始化
        let mut host = revm_interpreter::DummyHost::new(env);
        host.storage.insert(b256!("d2869508550c71a0ebfe05ddd28ce832b357803f6f387154b1a5451da28aca19").into(), U256::from(10000000000 as u64));
        host.storage.insert(b256!("ac0ab67043ecc9a2f17c6f6ba97786b2b1051a49d0101c2e2da0641d9a0e6da7").into(), U256::from(9900000000 as u64));
        
        
        let table = &revm_interpreter::opcode::make_instruction_table::<
            revm_interpreter::DummyHost,
            revm_primitives::CancunSpec,
        >();

        let mut interpreter = revm_interpreter::Interpreter::new(
            contract,
            gas_limit,
            false
        );

        // 执行并验证结果
        let action = interpreter.run(SharedMemory::new(), table, &mut host);
        assert!(
            interpreter.instruction_result.is_ok(),
            "Interpreter failed with {:?} for bench {}",
            interpreter.instruction_result,
            name
        );
        assert!(action.is_return(), "Interpreter bad action: {action:?} for bench {}", name);
    }
} 