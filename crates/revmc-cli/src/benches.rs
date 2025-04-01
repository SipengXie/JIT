use revmc::{primitives::hex, U256};

macro_rules! include_code_str {
    ($path:literal) => {
        crate::read_code_string(
            include_bytes!($path),
            std::path::Path::new($path).extension().and_then(|s| s.to_str()),
        )
    };
}

#[derive(Clone, Debug, Default)]
pub struct Bench {
    pub name: &'static str,
    pub bytecode: Vec<u8>,
    pub calldata: Vec<u8>,
    pub stack_input: Vec<U256>,
    pub native: Option<fn()>,
}

pub fn get_bench(name: &str) -> Option<Bench> {
    get_benches().into_iter().find(|b| b.name == name)
}

pub fn get_benches() -> Vec<Bench> {
    vec![
        Bench {
            name: "erc20_runtime",
            bytecode: include_code_str!("../../../data/erc20_runtime.rt.hex").unwrap(),
            calldata: hex!("40c10f1900000000000000000000000001010101010101010101010101010101010101010000000000000000000000000000000000000000000000000000000000010000")
            .to_vec(),
            ..Default::default()
        },
        Bench {
            name: "fibonacci_calldata",
            bytecode: include_code_str!("../../../data/fibonacci_calldata.rt.hex").unwrap(),
            calldata: hex!("c6c2ea1700000000000000000000000000000000000000000000000000000000000003e8")
            .to_vec(),
            ..Default::default()
        },
        Bench {
            name: "fibonacci_constant",
            bytecode: include_code_str!("../../../data/fibonacci_constant.rt.hex").unwrap(),
            calldata: hex!("9246aa9a").to_vec(),
            ..Default::default()
        },
        Bench {
            name: "factorial_calldata",
            bytecode: include_code_str!("../../../data/factorial_calldata.rt.hex").unwrap(),
            calldata: hex!("8371483400000000000000000000000000000000000000000000000000000000000003e8")
            .to_vec(),
            ..Default::default()
        },
        Bench {
            name: "factorial_constant",
            bytecode: include_code_str!("../../../data/factorial_constant.rt.hex").unwrap(),
            calldata: hex!("981111ef").to_vec(),
            ..Default::default()
        },
        Bench {
            name: "snailtracer",
            bytecode: include_code_str!("../../../data/snailtracer.rt.hex").unwrap(),
            calldata: hex!("30627b7c").to_vec(),
            ..Default::default()
        },
        Bench {
            name: "weth",
            bytecode: include_code_str!("../../../data/weth.rt.hex").unwrap(),
            calldata: hex!("6b7c477a").to_vec(),
            ..Default::default()
        },
        Bench {
            name: "hash_10k",
            bytecode: include_code_str!("../../../data/hash_10k.rt.hex").unwrap(),
            calldata: hex!("dc6bf8a7000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000021234000000000000000000000000000000000000000000000000000000000000").to_vec(),
            ..Default::default()
        },
        Bench {
            name: "uniswap_v2",
            bytecode: include_code_str!("../../../data/uniswap_v2.rt.hex").unwrap(),
            calldata: hex!("dfa5235e").to_vec(),
            ..Default::default()
        }
    ]
}
