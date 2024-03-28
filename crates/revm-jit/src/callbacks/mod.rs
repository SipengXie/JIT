use crate::{EvmContext, EvmWord};
use revm_interpreter::{
    as_usize_saturated, gas as rgas, CreateInputs, Gas, InstructionResult, InterpreterAction,
    InterpreterResult, SStoreResult,
};
use revm_jit_backend::{Attribute, TypeMethods};
use revm_primitives::{
    BerlinSpec, Bytes, CreateScheme, FrontierSpec, IstanbulSpec, SpecId, SpuriousDragonSpec,
    KECCAK_EMPTY, MAX_INITCODE_SIZE, U256,
};

#[macro_use]
mod macros;

callbacks! {
    |bcx| {
        let ptr = bcx.type_ptr();
        let usize = bcx.type_ptr_sized_int();
        let bool = bcx.type_int(1);
        let u8 = bcx.type_int(8);
        // let u64 = bcx.type_int(64);
    }

    Panic          = panic(ptr, usize) None,

    AddMod         = addmod(ptr) None,
    MulMod         = mulmod(ptr) None,
    Exp            = exp(ptr, ptr, u8) Some(u8),
    Keccak256      = keccak256(ptr, ptr) Some(u8),
    Balance        = balance(ptr, u8, usize) Some(u8),
    CallDataCopy   = calldatacopy(ptr, ptr) Some(u8),
    CodeCopy       = codecopy(ptr, ptr) Some(u8),
    ExtCodeSize    = extcodesize(ptr, u8, ptr) Some(u8),
    ExtCodeCopy    = extcodecopy(ptr, u8, ptr) Some(u8),
    ReturnDataCopy = returndatacopy(ptr, ptr) Some(u8),
    ExtCodeHash    = extcodehash(ptr) None,
    BlockHash      = blockhash(ptr, usize, ptr) Some(u8),
    SelfBalance    = self_balance(ptr, ptr) Some(u8),
    Mload          = mload(ptr, ptr) Some(u8),
    Mstore         = mstore(ptr, ptr) Some(u8),
    Mstore8        = mstore8(ptr, ptr) Some(u8),
    Sload          = sload(ptr, ptr) Some(u8),
    Sstore         = sstore(ptr, ptr) Some(u8),
    Msize          = msize(ptr) Some(usize),
    Tstore         = tstore(ptr, ptr) None,
    Tload          = tload(ptr, ptr) None,

    Create         = create(ptr, u8, ptr, bool) Some(u8),
    DoReturn       = do_return(ptr, ptr) Some(u8),
}

/* ------------------------------------- Callback Functions ------------------------------------- */
// NOTE: All functions MUST be `extern "C"` and their parameters must match the ones declared above.
//
// The `sp` parameter points to the top of the stack.
// `sp` functions are called with the length of the stack already checked and substracted.
// If results are expected to be pushed back onto the stack, they must be written to the read
// pointers in **reverse order**, meaning the last pointer is the first return value.

pub(crate) unsafe extern "C" fn panic(ptr: *const u8, len: usize) -> ! {
    let msg = std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len));
    panic!("{msg}");
}

pub(crate) unsafe extern "C" fn addmod(sp: *mut EvmWord) {
    read_words!(sp, a, b, c);
    *c = a.to_u256().add_mod(b.to_u256(), c.to_u256()).into();
}

pub(crate) unsafe extern "C" fn mulmod(sp: *mut EvmWord) {
    read_words!(sp, a, b, c);
    *c = a.to_u256().mul_mod(b.to_u256(), c.to_u256()).into();
}

pub(crate) unsafe extern "C" fn exp(
    ecx: &mut EvmContext<'_>,
    sp: *mut EvmWord,
    spec_id: SpecId,
) -> InstructionResult {
    read_words!(sp, base, exponent_ptr);
    let exponent = exponent_ptr.to_u256();
    // TODO: SpecId
    let gas = if spec_id.is_enabled_in(SpecId::SPURIOUS_DRAGON) {
        rgas::exp_cost::<SpuriousDragonSpec>(exponent)
    } else {
        rgas::exp_cost::<FrontierSpec>(exponent)
    };
    gas_opt!(ecx, gas);
    *exponent_ptr = base.to_u256().pow(exponent).into();
    InstructionResult::Continue
}

pub(crate) unsafe extern "C" fn keccak256(
    ecx: &mut EvmContext<'_>,
    sp: *mut EvmWord,
) -> InstructionResult {
    read_words!(sp, offset, len_ptr);
    if *len_ptr == EvmWord::ZERO {
        *len_ptr = EvmWord::from_be_bytes(KECCAK_EMPTY.0);
        gas!(ecx, rgas::KECCAK256);
        return InstructionResult::Continue;
    }
    let len = tri!(usize::try_from(*len_ptr));
    gas_opt!(ecx, rgas::keccak256_cost(len as u64));
    let offset = tri!(offset.try_into());

    resize_memory!(ecx, offset, len);

    let data = ecx.memory.slice(offset, len);
    *len_ptr = EvmWord::from_be_bytes(revm_primitives::keccak256(data).0);

    InstructionResult::Continue
}

pub(crate) unsafe extern "C" fn balance(
    ecx: &mut EvmContext<'_>,
    spec_id: SpecId,
    sp: *mut EvmWord,
) -> InstructionResult {
    let (balance, is_cold) = try_host!(ecx.host.balance(ecx.contract.address));
    *sp.sub(1) = balance.into();
    let gas = if spec_id.is_enabled_in(SpecId::BERLIN) {
        rgas::account_access_gas::<BerlinSpec>(is_cold)
    } else if spec_id.is_enabled_in(SpecId::ISTANBUL) {
        rgas::account_access_gas::<IstanbulSpec>(is_cold)
    } else if spec_id.is_enabled_in(SpecId::TANGERINE) {
        400
    } else {
        20
    };
    gas!(ecx, gas);
    InstructionResult::Continue
}

pub(crate) unsafe extern "C" fn calldatacopy(
    ecx: &mut EvmContext<'_>,
    sp: *mut EvmWord,
) -> InstructionResult {
    let input = decouple_lt(&ecx.contract.input[..]);
    copy_operation(ecx, sp, input)
}

pub(crate) unsafe extern "C" fn codecopy(
    ecx: &mut EvmContext<'_>,
    sp: *mut EvmWord,
) -> InstructionResult {
    let code = decouple_lt(ecx.contract.bytecode.original_bytecode_slice());
    copy_operation(ecx, sp, code)
}

pub(crate) unsafe extern "C" fn extcodesize(
    ecx: &mut EvmContext<'_>,
    spec_id: SpecId,
    sp: *mut EvmWord,
) -> InstructionResult {
    read_words!(sp, address);
    let (code, is_cold) = try_host!(ecx.host.code(address.to_address()));
    *address = code.len().into();
    let gas = if spec_id.is_enabled_in(SpecId::BERLIN) {
        if is_cold {
            rgas::COLD_ACCOUNT_ACCESS_COST
        } else {
            rgas::WARM_STORAGE_READ_COST
        }
    } else if spec_id.is_enabled_in(SpecId::TANGERINE) {
        700
    } else {
        20
    };
    gas!(ecx, gas);
    InstructionResult::Continue
}

pub(crate) unsafe extern "C" fn extcodecopy(
    ecx: &mut EvmContext<'_>,
    spec_id: SpecId,
    sp: *mut EvmWord,
) -> InstructionResult {
    read_words!(sp, address, memory_offset, code_offset, len);
    let (code, is_cold) = try_host!(ecx.host.code(address.to_address()));
    let len = tri!(usize::try_from(len));
    // TODO: SpecId
    // let gas = rgas::extcodecopy_cost::<SPEC>(len as u64, is_cold);
    let _ = spec_id;
    let _ = is_cold;
    if len != 0 {
        let memory_offset = tri!(memory_offset.try_into());
        let code_offset = code_offset.to_u256();
        let code_offset = as_usize_saturated!(code_offset).min(code.len());
        resize_memory!(ecx, memory_offset, len);
        ecx.memory.set_data(memory_offset, code_offset, len, code.bytes());
    }
    InstructionResult::Continue
}

pub(crate) unsafe extern "C" fn returndatacopy(
    ecx: &mut EvmContext<'_>,
    sp: *mut EvmWord,
) -> InstructionResult {
    read_words!(sp, memory_offset, offset, len);
    let len = tri!(usize::try_from(len));
    gas_opt!(ecx, rgas::verylowcopy_cost(len as u64));
    let data_offset = offset.to_u256();
    let data_offset = as_usize_saturated!(data_offset);
    let (data_end, overflow) = data_offset.overflowing_add(len);
    if overflow || data_end > ecx.return_data.len() {
        return InstructionResult::OutOfOffset;
    }
    if len != 0 {
        let memory_offset = tri!(memory_offset.try_into());
        resize_memory!(ecx, memory_offset, len);
        ecx.memory.set(memory_offset, &ecx.return_data[data_offset..data_end]);
    }
    InstructionResult::Continue
}

pub(crate) unsafe extern "C" fn extcodehash(
    ecx: &mut EvmContext<'_>,
    spec_id: SpecId,
    sp: *mut EvmWord,
) -> InstructionResult {
    read_words!(sp, address);
    let (hash, is_cold) = try_host!(ecx.host.code_hash(address.to_address()));
    *address = EvmWord::from_be_bytes(hash.0);
    let gas = if spec_id.is_enabled_in(SpecId::BERLIN) {
        if is_cold {
            rgas::COLD_ACCOUNT_ACCESS_COST
        } else {
            rgas::WARM_STORAGE_READ_COST
        }
    } else if spec_id.is_enabled_in(SpecId::ISTANBUL) {
        700
    } else {
        400
    };
    gas!(ecx, gas);
    InstructionResult::Continue
}

pub(crate) unsafe extern "C" fn blockhash(
    ecx: &mut EvmContext<'_>,
    number: usize,
    slot: *mut EvmWord,
) -> InstructionResult {
    let hash = try_host!(ecx.host.block_hash(U256::from(number)));
    *slot = EvmWord::from_be_bytes(hash.0);
    InstructionResult::Continue
}

pub(crate) unsafe extern "C" fn self_balance(
    ecx: &mut EvmContext<'_>,
    sp: *mut EvmWord,
) -> InstructionResult {
    read_words!(sp, slot);
    let (balance, _) = try_host!(ecx.host.balance(ecx.contract.address));
    *slot = balance.into();
    InstructionResult::Continue
}

pub(crate) unsafe extern "C" fn mload(
    ecx: &mut EvmContext<'_>,
    sp: *mut EvmWord,
) -> InstructionResult {
    read_words!(sp, offset_ptr);
    let offset = tri!(offset_ptr.try_into());
    resize_memory!(ecx, offset, 32);
    *offset_ptr = ecx.memory.get_u256(offset).into();
    InstructionResult::Continue
}

pub(crate) unsafe extern "C" fn mstore(
    ecx: &mut EvmContext<'_>,
    sp: *mut EvmWord,
) -> InstructionResult {
    read_words!(sp, offset, value);
    let offset = tri!(offset.try_into());
    resize_memory!(ecx, offset, 32);
    ecx.memory.set(offset, &value.to_be_bytes());
    InstructionResult::Continue
}

pub(crate) unsafe extern "C" fn mstore8(
    ecx: &mut EvmContext<'_>,
    sp: *mut EvmWord,
) -> InstructionResult {
    read_words!(sp, offset, value);
    let offset = tri!(offset.try_into());
    resize_memory!(ecx, offset, 1);
    ecx.memory.set_byte(offset, value.to_u256().byte(0));
    InstructionResult::Continue
}

pub(crate) unsafe extern "C" fn sload(
    ecx: &mut EvmContext<'_>,
    sp: *mut EvmWord,
) -> InstructionResult {
    read_words!(sp, index);
    let address = ecx.contract.address;
    let (res, is_cold) = try_opt!(ecx.host.sload(address, index.to_u256()));
    *index = res.into();
    // TODO: SpecId
    let _ = is_cold;
    // rgas::sload_cost(is_cold)
    InstructionResult::Continue
}

pub(crate) unsafe extern "C" fn sstore(
    ecx: &mut EvmContext<'_>,
    sp: *mut EvmWord,
) -> InstructionResult {
    read_words!(sp, index, value);
    let SStoreResult { original_value: original, present_value: old, new_value: new, is_cold } =
        try_opt!(ecx.host.sstore(ecx.contract.address, index.to_u256(), value.to_u256()));

    let _ = original;
    let _ = old;
    let _ = new;
    let _ = is_cold;

    // TODO: SpecId, refund
    // rgas::sstore_refund::<SPEC>(original, old, new)
    // TODO: SpecId
    // rgas::sstore_cost::<SPEC>(original, old, new, remaining_gas, is_cold)
    InstructionResult::Continue
}

// TODO: Inline and remove.
pub(crate) unsafe extern "C" fn msize(ecx: &mut EvmContext<'_>) -> usize {
    ecx.memory.len()
}

pub(crate) unsafe extern "C" fn tstore(ecx: &mut EvmContext<'_>, sp: *mut EvmWord) {
    read_words!(sp, key, value);
    ecx.host.tstore(ecx.contract.address, key.to_u256(), value.to_u256());
}

pub(crate) unsafe extern "C" fn tload(ecx: &mut EvmContext<'_>, sp: *mut EvmWord) {
    read_words!(sp, key);
    *key = ecx.host.tload(ecx.contract.address, key.to_u256()).into();
}

pub(crate) unsafe extern "C" fn create(
    ecx: &mut EvmContext<'_>,
    spec_id: SpecId,
    sp: *mut EvmWord,
    is_create2: bool,
) -> InstructionResult {
    read_words!(sp, value, code_offset, len, salt);
    let len = tri!(usize::try_from(len));
    let code = if len != 0 {
        if spec_id.is_enabled_in(SpecId::SHANGHAI) {
            // Limit is set as double of max contract bytecode size
            let max_initcode_size = ecx
                .host
                .env()
                .cfg
                .limit_contract_code_size
                .map(|limit| limit.saturating_mul(2))
                .unwrap_or(MAX_INITCODE_SIZE);
            if len > max_initcode_size {
                return InstructionResult::CreateInitCodeSizeLimit;
            }
            gas!(ecx, rgas::initcode_cost(len as u64));
        }

        let code_offset = tri!(code_offset.try_into());
        resize_memory!(ecx, code_offset, len);
        Bytes::copy_from_slice(ecx.memory.slice(code_offset, len))
    } else {
        Bytes::new()
    };

    gas_opt!(ecx, if is_create2 { rgas::create2_cost(len) } else { Some(rgas::CREATE) });

    let scheme = if is_create2 {
        CreateScheme::Create2 { salt: salt.to_u256() }
    } else {
        CreateScheme::Create
    };

    let mut gas_limit = ecx.gas.remaining();
    if spec_id.is_enabled_in(SpecId::TANGERINE) {
        gas_limit -= gas_limit / 64;
    }
    gas!(ecx, gas_limit);

    *ecx.next_action = InterpreterAction::Create {
        inputs: Box::new(CreateInputs {
            caller: ecx.contract.address,
            scheme,
            value: value.to_u256(),
            init_code: code,
            gas_limit,
        }),
    };

    InstructionResult::Continue
}

pub(crate) unsafe extern "C" fn do_return(
    ecx: &mut EvmContext<'_>,
    offset: *mut EvmWord,
    result: InstructionResult,
) -> InstructionResult {
    read_words!(offset, offset, len);
    let len = tri!(len.try_into());
    let output = if len != 0 {
        let offset = tri!(offset.try_into());
        resize_memory!(ecx, offset, len);
        ecx.memory.slice(offset, len).to_vec().into()
    } else {
        Bytes::new()
    };
    *ecx.next_action = InterpreterAction::Return {
        result: InterpreterResult {
            output,
            gas: Gas::new(0), // TODO
            result,
        },
    };
    InstructionResult::Continue
}

// --- utils ---

/// Splits the stack pointer into `N` elements by casting it to an array.
/// This has the same effect as popping `N` elements from the stack since the JIT function
/// has already modified the length.
///
/// NOTE: this returns the arguments in **reverse order**. Use [`read_words!`] to get them in order.
///
/// The returned lifetime is valid for the entire duration of the callback.
///
/// # Safety
///
/// Caller must ensure that `N` matches the number of elements popped in JIT code.
#[inline(always)]
unsafe fn read_words_rev<'a, const N: usize>(sp: *mut EvmWord) -> &'a mut [EvmWord; N] {
    &mut *sp.sub(N).cast::<[EvmWord; N]>()
}

#[inline]
fn resize_memory(ecx: &mut EvmContext<'_>, offset: usize, len: usize) -> InstructionResult {
    let size = offset.saturating_add(len);
    if size > ecx.memory.len() {
        let rounded_size = revm_interpreter::interpreter::next_multiple_of_32(size);

        // TODO: Memory limit

        // TODO: try_resize
        gas!(ecx, rgas::memory_gas(rounded_size / 32));
        ecx.memory.resize(rounded_size);
    }
    InstructionResult::Continue
}

#[inline]
fn copy_operation(ecx: &mut EvmContext<'_>, sp: *mut EvmWord, data: &[u8]) -> InstructionResult {
    read_words!(sp, memory_offset, data_offset, len);
    let len = tri!(usize::try_from(len));
    gas_opt!(ecx, rgas::verylowcopy_cost(len as u64));
    if len == 0 {
        return InstructionResult::Continue;
    }
    let memory_offset = tri!(memory_offset.try_into());
    resize_memory!(ecx, memory_offset, len);
    let data_offset = data_offset.to_u256();
    let data_offset = as_usize_saturated!(data_offset);
    ecx.memory.set_data(memory_offset, data_offset, len, data);
    InstructionResult::Continue
}

unsafe fn decouple_lt<'b, T: ?Sized>(x: &T) -> &'b T {
    std::mem::transmute(x)
}
