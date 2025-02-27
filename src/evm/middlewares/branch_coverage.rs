use std::collections::{HashMap, HashSet};
use std::fmt::{Debug};
use std::fs::OpenOptions;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};
use itertools::Itertools;
use libafl::inputs::Input;
use libafl::prelude::{HasCorpus, HasMetadata, State};
use revm_interpreter::Interpreter;
use revm_primitives::Bytecode;
use crate::evm::host::FuzzHost;
use crate::evm::input::{ConciseEVMInput, EVMInput, EVMInputT};
use crate::evm::middlewares::middleware::{Middleware, MiddlewareType};
use crate::generic_vm::vm_state::VMStateT;
use crate::input::VMInputT;
use crate::state::{HasCaller, HasCurrentInputIdx, HasItyState};
use crate::evm::types::{as_u64, EVMAddress};
use crate::evm::types::ProjectSourceMapTy;

pub fn branch_pc(bytecode: &Bytecode) -> (usize, usize) {
    let mut JUMPCount = 0;
    let mut JUMPICount = 0;
    let mut i = 0;
    let bytes = bytecode.bytes();

    while i < bytes.len() {
        let op = *bytes.get(i).unwrap();
        i += 1;
        /// stip off the PUSH XXXxxxxxxXXX instruction
        if op >= 0x60 && op <= 0x7f {
            i += op as usize - 0x5f;
            continue;
        }

        match op {
            0x56 => JUMPCount += 1,
            0x57 => JUMPICount += 2,
            _ => (),
        }
    }
    (JUMPCount, JUMPICount)
}

#[derive(Clone, Debug)]
pub struct BranchCoverage {
    pub pc_coverage: HashMap<EVMAddress, HashSet<usize>>,
    pub total_instr: HashMap<EVMAddress, usize>,
    pub total_instr_set: HashMap<EVMAddress, HashSet<usize>>,
    pub total_jump_branch: HashMap<EVMAddress, usize>,
    pub total_jumpi_branch: HashMap<EVMAddress, usize>,
    pub work_dir: String,
}


impl BranchCoverage {
    pub fn new() -> Self {
        Self {
            pc_coverage: HashMap::new(),
            total_instr: HashMap::new(),
            total_instr_set: HashMap::new(),
            total_jump_branch: HashMap::new(),
            total_jumpi_branch: HashMap::new(),
            work_dir: "work_dir".to_string(),
        }
    }

    pub fn record_branch_coverage(&mut self, source_map: &ProjectSourceMapTy) {
        /*
        println!("total_instr: {:?}", self.total_instr);
        println!("total_instr_set: {:?}", self.total_instr_set);
        println!("pc_coverage: {:?}",  self.pc_coverage);
        println!("total_jump_branch: {:?}", self.total_jump_branch);
        println!("total_jumpi_branch: {:?}", self.total_jumpi_branch);
         */

        let mut data = format!(
            "===================Branch Coverage Report =================== \n{}",
            self.total_instr
                .keys()
                .map(|k| {
                    let total = self.total_jump_branch.get(k).unwrap() + self.total_jumpi_branch.get(k).unwrap();
                    let cov = self.total_instr.get(k).unwrap();
                    let mut per = 0.0;
                    if total == 0 {
                        per = 100.0;
                    }else {
                        per = *cov as f64 / total as f64 * 100.0;
                    }
                    format!("Contract: {:?}, format Coverage: {} / {} ({:.2}%)",
                            k,
                            *cov,
                            total,
                            per
                    )
                })
                .join("\n")
        );

        println!("\n\n{}", data);

        let mut file = OpenOptions::new()
            .write(true)
            .append(false)
            .create(true)
            .open(format!("{}/branch_cov_{}.txt", self.work_dir, SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()))
            .unwrap();
        file.write_all(data.as_bytes()).unwrap();
    }
}


impl<I, VS, S> Middleware<VS, I, S> for BranchCoverage
    where
        I: Input + VMInputT<VS, EVMAddress, EVMAddress, ConciseEVMInput> + EVMInputT + 'static,
        VS: VMStateT,
        S: State
        + HasCaller<EVMAddress>
        + HasCorpus<I>
        + HasItyState<EVMAddress, EVMAddress, VS, ConciseEVMInput>
        + HasMetadata
        + HasCurrentInputIdx
        + Debug
        + Clone,
{
    unsafe fn on_step(
        &mut self,
        interp: &mut Interpreter,
        host: &mut FuzzHost<VS, I, S>,
        state: &mut S,
    ) {
        let address = interp.contract.address;
        let pc = interp.program_counter().clone();
        let mut is_insert = false;
        let mut is_insert_jumpi = false;
        let mut total_brash = 1;
        let mut jmppc: usize = 0;
        self.pc_coverage.entry(address).or_default().insert(pc);
        match *interp.instruction_pointer {
            0x56 => { // JUMP
                // println!("JUMPI: {:#X} {:?}, {:#X}", pc,  address, as_u64(interp.stack.peek(0).unwrap()) as usize);
                if self.total_instr_set.get(&address).is_none() {
                    is_insert = true;
                } else if  !self.total_instr_set.get(&address).unwrap().contains(&pc) {
                    total_brash = self.total_instr.get(&address).unwrap()+1;
                    is_insert = true;
                }
            }
            0x57 => { // JUMPI
                // println!("JUMPI: {:#X} {:?}, {:#X}", pc,  address, as_u64(interp.stack.peek(0).unwrap()) as usize);
                jmppc = as_u64(interp.stack.peek(0).unwrap()) as usize;
                if self.total_instr_set.get(&address).is_none(){
                    is_insert = true;
                    is_insert_jumpi = true;
                    total_brash = 2;
                }else{
                    total_brash = self.total_instr.get(&address).unwrap()+2;
                    if !self.total_instr_set.get(&address).unwrap().contains(&pc){
                        is_insert = true;
                    }
                    if !self.total_instr_set.get(&address).unwrap().contains(&jmppc) {
                        is_insert_jumpi = true;
                    }
                }

            }
            _ => {
            }
        }

        if is_insert {
            let total = self.total_instr.entry(address).or_insert(0);
            *total = total_brash;
            self.total_instr_set.entry(address).or_insert(HashSet::new()).insert(pc);
        }
        if is_insert_jumpi {
            self.total_instr_set.entry(address).or_insert(HashSet::new()).insert(jmppc);
            if !is_insert {
                let total = self.total_instr.entry(address).or_insert(0);
                *total = total_brash;
            }
        }


    }

    unsafe fn on_insert(&mut self, bytecode: &mut Bytecode, address: EVMAddress, host: &mut FuzzHost<VS, I, S>, state: &mut S) {
        // println!("on_insert: {:#X} {:?}", address, hex::encode(bytecode.clone().bytecode.as_ref()));
        self.work_dir = host.work_dir.clone();
        let total = branch_pc(&bytecode.clone());
        self.total_jump_branch.insert(address, total.0);
        self.total_jumpi_branch.insert(address, total.1);
    }

    fn get_type(&self) -> MiddlewareType {
        MiddlewareType::BranchCoverage
    }
}


mod tests {
    use bytes::Bytes;
    use super::*;

    #[test]
    fn test_branchs_pc() {
        let pcs = branch_pc(&Bytecode::new_raw(
            Bytes::from(
                hex::decode("60806040526004361061004e5760003560e01c80632d2c55651461008d578063819d4cc6146100de5780638980f11f146101005780638b21f170146101205780639342c8f41461015457600080fd5b36610088576040513481527f27f12abfe35860a9a927b465bb3d4a9c23c8428174b83f278fe45ed7b4da26629060200160405180910390a1005b600080fd5b34801561009957600080fd5b506100c17f0000000000000000000000003e40d73eb977dc6a537af587d48316fee66e9c8c81565b6040516001600160a01b0390911681526020015b60405180910390f35b3480156100ea57600080fd5b506100fe6100f93660046106bb565b610182565b005b34801561010c57600080fd5b506100fe61011b3660046106bb565b61024e565b34801561012c57600080fd5b506100c17f000000000000000000000000ae7ab96520de3a18e5e111b5eaab095312d7fe8481565b34801561016057600080fd5b5061017461016f3660046106f3565b610312565b6040519081526020016100d5565b6040518181526001600160a01b0383169033907f6a30e6784464f0d1f4158aa4cb65ae9239b0fa87c7f2c083ee6dde44ba97b5e69060200160405180910390a36040516323b872dd60e01b81523060048201526001600160a01b037f0000000000000000000000003e40d73eb977dc6a537af587d48316fee66e9c8c81166024830152604482018390528316906323b872dd90606401600060405180830381600087803b15801561023257600080fd5b505af1158015610246573d6000803e3d6000fd5b505050505050565b6000811161029a5760405162461bcd60e51b815260206004820152601460248201527316915493d7d49150d3d591549657d05353d5539560621b60448201526064015b60405180910390fd5b6040518181526001600160a01b0383169033907faca8fb252cde442184e5f10e0f2e6e4029e8cd7717cae63559079610702436aa9060200160405180910390a361030e6001600160a01b0383167f0000000000000000000000003e40d73eb977dc6a537af587d48316fee66e9c8c83610418565b5050565b6000336001600160a01b037f000000000000000000000000ae7ab96520de3a18e5e111b5eaab095312d7fe8416146103855760405162461bcd60e51b81526020600482015260166024820152754f4e4c595f4c49444f5f43414e5f574954484452415760501b6044820152606401610291565b478281116103935780610395565b825b91508115610412577f000000000000000000000000ae7ab96520de3a18e5e111b5eaab095312d7fe846001600160a01b0316634ad509b2836040518263ffffffff1660e01b81526004016000604051808303818588803b1580156103f857600080fd5b505af115801561040c573d6000803e3d6000fd5b50505050505b50919050565b604080516001600160a01b038416602482015260448082018490528251808303909101815260649091019091526020810180516001600160e01b031663a9059cbb60e01b17905261046a90849061046f565b505050565b60006104c4826040518060400160405280602081526020017f5361666545524332303a206c6f772d6c6576656c2063616c6c206661696c6564815250856001600160a01b03166105419092919063ffffffff16565b80519091501561046a57808060200190518101906104e2919061070c565b61046a5760405162461bcd60e51b815260206004820152602a60248201527f5361666545524332303a204552433230206f7065726174696f6e20646964206e6044820152691bdd081cdd58d8d9595960b21b6064820152608401610291565b6060610550848460008561055a565b90505b9392505050565b6060824710156105bb5760405162461bcd60e51b815260206004820152602660248201527f416464726573733a20696e73756666696369656e742062616c616e636520666f6044820152651c8818d85b1b60d21b6064820152608401610291565b843b6106095760405162461bcd60e51b815260206004820152601d60248201527f416464726573733a2063616c6c20746f206e6f6e2d636f6e74726163740000006044820152606401610291565b600080866001600160a01b03168587604051610625919061075e565b60006040518083038185875af1925050503d8060008114610662576040519150601f19603f3d011682016040523d82523d6000602084013e610667565b606091505b5091509150610677828286610682565b979650505050505050565b60608315610691575081610553565b8251156106a15782518084602001fd5b8160405162461bcd60e51b8152600401610291919061077a565b600080604083850312156106ce57600080fd5b82356001600160a01b03811681146106e557600080fd5b946020939093013593505050565b60006020828403121561070557600080fd5b5035919050565b60006020828403121561071e57600080fd5b8151801515811461055357600080fd5b60005b83811015610749578181015183820152602001610731565b83811115610758576000848401525b50505050565b6000825161077081846020870161072e565b9190910192915050565b602081526000825180602084015261079981604085016020870161072e565b601f01601f1916919091016040019291505056fea2646970667358221220c0f03149dd58fa21e9bfb72a010b74b1e518d704a2d63d8cc44c0ad3a2f573da64736f6c63430008090033").unwrap()
            )
        ));

        assert_eq!(pcs.0, 38);
        assert_eq!(pcs.1, 68);

    }
}
