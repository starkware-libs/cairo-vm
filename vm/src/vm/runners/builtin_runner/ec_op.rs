use crate::air_private_input::{PrivateInput, PrivateInputEcOp};
use crate::types::instance_definitions::ec_op_instance_def::{
    CELLS_PER_EC_OP, INPUT_CELLS_PER_EC_OP, SCALAR_HEIGHT,
};
use crate::types::relocatable::{MaybeRelocatable, Relocatable};
use crate::vm::errors::memory_errors::MemoryError;
use crate::vm::errors::runner_errors::RunnerError;
use crate::vm::vm_memory::memory::Memory;
use crate::vm::vm_memory::memory_segments::MemorySegmentManager;
use crate::Felt252;
use num_integer::{div_ceil, Integer};
use starknet_types_core::curve::ProjectivePoint;
use std::{cell::RefCell, collections::HashMap};

#[derive(Debug, Clone)]
pub struct EcOpBuiltinRunner {
    ratio: Option<u32>,
    pub base: usize,
    pub(crate) stop_ptr: Option<usize>,
    pub(crate) included: bool,
    cache: RefCell<HashMap<Relocatable, Felt252>>,
}

impl EcOpBuiltinRunner {
    pub fn new(ratio: Option<u32>, included: bool) -> Self {
        EcOpBuiltinRunner {
            base: 0,
            ratio,
            stop_ptr: None,
            included,
            cache: RefCell::new(HashMap::new()),
        }
    }
    ///Returns True if the point (x, y) is on the elliptic curve defined as
    ///y^2 = x^3 + alpha * x + beta (mod p)
    ///or False otherwise.
    fn point_on_curve(x: &Felt252, y: &Felt252, alpha: &Felt252, beta: &Felt252) -> bool {
        y.pow(2_u32) == (x.pow(3_u32) + alpha * x) + beta
    }

    ///Returns the result of the EC operation P + m * Q.
    /// where P = (p_x, p_y), Q = (q_x, q_y) are points on the elliptic curve defined as
    /// y^2 = x^3 + alpha * x + beta (mod prime).
    /// Mimics the operation of the AIR, so that this function fails whenever the builtin AIR
    /// would not yield a correct result, i.e. when any part of the computation attempts to add
    /// two points with the same x coordinate.
    fn ec_op_impl(
        partial_sum: (Felt252, Felt252),
        doubled_point: (Felt252, Felt252),
        m: &Felt252,
        height: u32,
    ) -> Result<(Felt252, Felt252), RunnerError> {
        let slope = m.to_biguint();
        let mut partial_sum_b = ProjectivePoint::from_affine(partial_sum.0, partial_sum.1)
            .map_err(|_| RunnerError::PointNotOnCurve(Box::new(partial_sum)))?;
        let mut doubled_point_b = ProjectivePoint::from_affine(doubled_point.0, doubled_point.1)
            .map_err(|_| RunnerError::PointNotOnCurve(Box::new(doubled_point)))?;
        for i in 0..(height as u64).min(slope.bits()) {
            if partial_sum_b.x() * doubled_point_b.z() == partial_sum_b.z() * doubled_point_b.x() {
                return Err(RunnerError::EcOpSameXCoordinate(
                    Self::format_ec_op_error(partial_sum_b, slope, doubled_point_b)
                        .into_boxed_str(),
                ));
            };
            if slope.bit(i) {
                partial_sum_b += &doubled_point_b;
            }
            doubled_point_b = doubled_point_b.double();
        }
        partial_sum_b
            .to_affine()
            .map(|p| (p.x(), p.y()))
            .map_err(|_| RunnerError::InvalidPoint)
    }

    pub fn initialize_segments(&mut self, segments: &mut MemorySegmentManager) {
        self.base = segments.add().segment_index as usize // segments.add() always returns a positive index
    }

    pub fn initial_stack(&self) -> Vec<MaybeRelocatable> {
        if self.included {
            vec![MaybeRelocatable::from((self.base as isize, 0))]
        } else {
            vec![]
        }
    }

    pub fn base(&self) -> usize {
        self.base
    }

    pub fn ratio(&self) -> Option<u32> {
        self.ratio
    }

    pub fn deduce_memory_cell(
        &self,
        address: Relocatable,
        memory: &Memory,
    ) -> Result<Option<MaybeRelocatable>, RunnerError> {
        //Constant values declared here
        const EC_POINT_INDICES: [(usize, usize); 3] = [(0, 1), (2, 3), (5, 6)];
        const OUTPUT_INDICES: (usize, usize) = EC_POINT_INDICES[2];
        let alpha: Felt252 = Felt252::ONE;
        let beta_low: Felt252 = Felt252::from(0x609ad26c15c915c1f4cdfcb99cee9e89_u128);
        let beta_high: Felt252 = Felt252::from(0x6f21413efbe40de150e596d72f7a8c5_u128);
        let beta: Felt252 = (beta_high * (Felt252::ONE + Felt252::from(u128::MAX))) + beta_low;

        let index = address.offset.mod_floor(&(CELLS_PER_EC_OP as usize));
        //Index should be an output cell
        if index != OUTPUT_INDICES.0 && index != OUTPUT_INDICES.1 {
            return Ok(None);
        }
        let instance = Relocatable::from((address.segment_index, address.offset - index));
        let x_addr = (instance + (&Felt252::from(INPUT_CELLS_PER_EC_OP)))
            .map_err(|_| RunnerError::Memory(MemoryError::ExpectedInteger(Box::new(instance))))?;

        if let Some(number) = self.cache.borrow().get(&address).cloned() {
            return Ok(Some(MaybeRelocatable::Int(number)));
        }

        //All input cells should be filled, and be integer values
        //If an input cell is not filled, return None
        let mut input_cells = Vec::<Felt252>::with_capacity(INPUT_CELLS_PER_EC_OP as usize);
        for i in 0..INPUT_CELLS_PER_EC_OP as usize {
            match memory.get(&(instance + i)?) {
                None => return Ok(None),
                Some(MaybeRelocatable::Int(num)) => {
                    input_cells.push(num);
                }
                Some(_) => {
                    return Err(RunnerError::Memory(MemoryError::ExpectedInteger(Box::new(
                        (instance + i)?,
                    ))))
                }
            };
        }
        //Assert that m is under the limit defined by scalar_limit.
        /*if input_cells[M_INDEX].as_ref() >= &self.ec_op_builtin.scalar_limit {
            return Err(RunnerError::EcOpBuiltinScalarLimit(
                self.ec_op_builtin.scalar_limit.clone(),
            ));
        }*/

        // Assert that if the current address is part of a point, the point is on the curve
        for pair in &EC_POINT_INDICES[0..2] {
            if !EcOpBuiltinRunner::point_on_curve(
                &input_cells[pair.0],
                &input_cells[pair.1],
                &alpha,
                &beta,
            ) {
                return Err(RunnerError::PointNotOnCurve(Box::new((
                    input_cells[pair.0],
                    input_cells[pair.1],
                ))));
            };
        }
        let result = EcOpBuiltinRunner::ec_op_impl(
            (input_cells[0].to_owned(), input_cells[1].to_owned()),
            (input_cells[2].to_owned(), input_cells[3].to_owned()),
            &input_cells[4],
            SCALAR_HEIGHT,
        )?;
        self.cache.borrow_mut().insert(x_addr, result.0);
        self.cache.borrow_mut().insert(
            (x_addr + 1usize)
                .map_err(|_| RunnerError::Memory(MemoryError::ExpectedInteger(Box::new(x_addr))))?,
            result.1,
        );
        match index - INPUT_CELLS_PER_EC_OP as usize {
            0 => Ok(Some(MaybeRelocatable::Int(result.0))),
            _ => Ok(Some(MaybeRelocatable::Int(result.1))),
            //Default case corresponds to 1, as there are no other possible cases
        }
    }

    pub fn get_used_cells(&self, segments: &MemorySegmentManager) -> Result<usize, MemoryError> {
        segments
            .get_segment_used_size(self.base())
            .ok_or(MemoryError::MissingSegmentUsedSizes)
    }

    pub fn get_used_instances(
        &self,
        segments: &MemorySegmentManager,
    ) -> Result<usize, MemoryError> {
        let used_cells = self.get_used_cells(segments)?;
        Ok(div_ceil(used_cells, CELLS_PER_EC_OP as usize))
    }

    pub fn format_ec_op_error(
        p: ProjectivePoint,
        m: num_bigint::BigUint,
        q: ProjectivePoint,
    ) -> String {
        let p = p.to_affine().map(|p| (p.x(), p.y())).unwrap_or_default();
        let q = q.to_affine().map(|q| (q.x(), q.y())).unwrap_or_default();
        format!("Cannot apply EC operation: computation reached two points with the same x coordinate. \n
    Attempting to compute P + m * Q where:\n
    P = {p:?} \n
    m = {m:?}\n
    Q = {q:?}.")
    }

    pub fn air_private_input(&self, memory: &Memory) -> Vec<PrivateInput> {
        let mut private_inputs = vec![];
        if let Some(segment) = memory.data.get(self.base) {
            let segment_len = segment.len();
            for (index, off) in (0..segment_len)
                .step_by(CELLS_PER_EC_OP as usize)
                .enumerate()
            {
                // Add the input cells of each ec_op instance to the private inputs
                if let (Ok(p_x), Ok(p_y), Ok(q_x), Ok(q_y), Ok(m)) = (
                    memory.get_integer((self.base as isize, off).into()),
                    memory.get_integer((self.base as isize, off + 1).into()),
                    memory.get_integer((self.base as isize, off + 2).into()),
                    memory.get_integer((self.base as isize, off + 3).into()),
                    memory.get_integer((self.base as isize, off + 4).into()),
                ) {
                    private_inputs.push(PrivateInput::EcOp(PrivateInputEcOp {
                        index,
                        p_x: *p_x,
                        p_y: *p_y,
                        m: *m,
                        q_x: *q_x,
                        q_y: *q_y,
                    }))
                }
            }
        }
        private_inputs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hint_processor::builtin_hint_processor::builtin_hint_processor_definition::BuiltinHintProcessor;
    use crate::types::builtin_name::BuiltinName;
    use crate::types::layout_name::LayoutName;
    use crate::types::program::Program;
    use crate::utils::test_utils::*;
    use crate::vm::errors::cairo_run_errors::CairoRunError;
    use crate::vm::errors::vm_errors::VirtualMachineError;
    use crate::{felt_hex, felt_str, relocatable};

    use crate::vm::{
        errors::{memory_errors::MemoryError, runner_errors::RunnerError},
        runners::builtin_runner::BuiltinRunner,
    };
    use EcOpBuiltinRunner;

    #[test]
    fn get_used_instances() {
        let builtin = EcOpBuiltinRunner::new(Some(10), true);

        let mut vm = vm!();
        vm.segments.segment_used_sizes = Some(vec![1]);

        assert_eq!(builtin.get_used_instances(&vm.segments), Ok(1));
    }

    #[test]
    fn final_stack() {
        let mut builtin: BuiltinRunner = EcOpBuiltinRunner::new(Some(10), true).into();

        let mut vm = vm!();

        vm.segments = segments![
            ((0, 0), (0, 0)),
            ((0, 1), (0, 1)),
            ((2, 0), (0, 0)),
            ((2, 1), (0, 0))
        ];

        vm.segments.segment_used_sizes = Some(vec![0]);

        let pointer = Relocatable::from((2, 2));

        assert_eq!(
            builtin.final_stack(&vm.segments, pointer).unwrap(),
            Relocatable::from((2, 1))
        );
    }

    #[test]
    fn final_stack_error_stop_pointer() {
        let mut builtin: BuiltinRunner = EcOpBuiltinRunner::new(Some(10), true).into();

        let mut vm = vm!();

        vm.segments = segments![
            ((0, 0), (0, 0)),
            ((0, 1), (0, 1)),
            ((2, 0), (0, 0)),
            ((2, 1), (0, 0))
        ];

        vm.segments.segment_used_sizes = Some(vec![994]);

        let pointer = Relocatable::from((2, 2));

        assert_eq!(
            builtin.final_stack(&vm.segments, pointer),
            Err(RunnerError::InvalidStopPointer(Box::new((
                BuiltinName::ec_op,
                relocatable!(0, 994),
                relocatable!(0, 0)
            ))))
        );
    }

    #[test]
    fn final_stack_error_when_notincluded() {
        let mut builtin: BuiltinRunner = EcOpBuiltinRunner::new(Some(10), false).into();

        let mut vm = vm!();

        vm.segments = segments![
            ((0, 0), (0, 0)),
            ((0, 1), (0, 1)),
            ((2, 0), (0, 0)),
            ((2, 1), (0, 0))
        ];

        vm.segments.segment_used_sizes = Some(vec![0]);

        let pointer = Relocatable::from((2, 2));

        assert_eq!(
            builtin.final_stack(&vm.segments, pointer).unwrap(),
            Relocatable::from((2, 2))
        );
    }

    #[test]
    fn final_stack_error_non_relocatable() {
        let mut builtin: BuiltinRunner = EcOpBuiltinRunner::new(Some(10), true).into();

        let mut vm = vm!();

        vm.segments = segments![
            ((0, 0), (0, 0)),
            ((0, 1), (0, 1)),
            ((2, 0), (0, 0)),
            ((2, 1), 2)
        ];

        vm.segments.segment_used_sizes = Some(vec![0]);

        let pointer = Relocatable::from((2, 2));

        assert_eq!(
            builtin.final_stack(&vm.segments, pointer),
            Err(RunnerError::NoStopPointer(Box::new(BuiltinName::ec_op)))
        );
    }

    #[test]
    fn get_used_cells_and_allocated_size_test() {
        let builtin: BuiltinRunner = EcOpBuiltinRunner::new(Some(10), true).into();

        let program = program!(
            builtins = vec![BuiltinName::pedersen],
            data = vec_data!(
                (4612671182993129469_i64),
                (5189976364521848832_i64),
                (18446744073709551615_i128),
                (5199546496550207487_i64),
                (4612389712311386111_i64),
                (5198983563776393216_i64),
                (2),
                (2345108766317314046_i64),
                (5191102247248822272_i64),
                (5189976364521848832_i64),
                (7),
                (1226245742482522112_i64),
                ((
                    "3618502788666131213697322783095070105623107215331596699973092056135872020470",
                    10
                )),
                (2345108766317314046_i64)
            ),
            main = Some(8),
        );
        let mut cairo_runner = cairo_runner!(program);

        cairo_runner.vm.segments.segment_used_sizes = Some(vec![0]);

        let mut hint_processor = BuiltinHintProcessor::new_empty();

        let address = cairo_runner.initialize(false).unwrap();

        cairo_runner
            .run_until_pc(address, &mut hint_processor)
            .unwrap();

        assert_eq!(
            builtin.get_used_cells_and_allocated_size(&cairo_runner.vm),
            Ok((0, 7))
        );
    }

    #[test]
    fn get_allocated_memory_units() {
        let builtin: BuiltinRunner = EcOpBuiltinRunner::new(Some(10), true).into();

        let program = program!(
            builtins = vec![BuiltinName::ec_op],
            data = vec_data!(
                (4612671182993129469_i64),
                (5189976364521848832_i64),
                (18446744073709551615_i128),
                (5199546496550207487_i64),
                (4612389712311386111_i64),
                (5198983563776393216_i64),
                (2),
                (2345108766317314046_i64),
                (5191102247248822272_i64),
                (5189976364521848832_i64),
                (7),
                (1226245742482522112_i64),
                ((
                    "3618502788666131213697322783095070105623107215331596699973092056135872020470",
                    10
                )),
                (2345108766317314046_i64)
            ),
            main = Some(8),
        );

        let mut cairo_runner = cairo_runner!(program);

        let mut hint_processor = BuiltinHintProcessor::new_empty();

        let address = cairo_runner.initialize(false).unwrap();

        cairo_runner
            .run_until_pc(address, &mut hint_processor)
            .unwrap();

        assert_eq!(builtin.get_allocated_memory_units(&cairo_runner.vm), Ok(7));
    }

    #[test]
    fn point_is_on_curve_a() {
        let x = felt_hex!("0x1ef15c18599971b7beced415a40f0c7deacfd9b0d1819e03d723d8bc943cfca");
        let y = felt_hex!("0x5668060aa49730b7be4801df46ec62de53ecd11abe43a32873000c36e8dc1f");
        let alpha = Felt252::ONE;
        let beta = felt_hex!("0x6f21413efbe40de150e596d72f7a8c5609ad26c15c915c1f4cdfcb99cee9e89");
        assert!(EcOpBuiltinRunner::point_on_curve(&x, &y, &alpha, &beta));
    }

    #[test]
    fn point_is_on_curve_b() {
        let x = felt_hex!("0x6f0a1ddaf19c44781c8946db396f494a10ffab183c2d8cf6c4cd321a8d87fd9");
        let y = felt_hex!("0x4afa52a9ef8c023d3385fddb6e1d78d57b0693b9b02d45d0f939b526d474c39");
        let alpha = Felt252::ONE;
        let beta = felt_hex!("0x6f21413efbe40de150e596d72f7a8c5609ad26c15c915c1f4cdfcb99cee9e89");
        assert!(EcOpBuiltinRunner::point_on_curve(&x, &y, &alpha, &beta));
    }

    #[test]
    fn point_is_not_on_curve_a() {
        let x = felt_hex!("0x1ef15c1a2162fb0d2e5d83196a6fb0509632fab5d746f0c3d723d8bc943cfca");
        let y = felt_hex!("0x5668060aa49730b7be4801df46ec62de53ecd11abe43a32873000c36e8dc1f");
        let alpha = Felt252::ONE;
        let beta = felt_hex!("0x6f21413efbe40de150e596d72f7a8c5609ad26c15c915c1f4cdfcb99cee9e89");
        assert!(!EcOpBuiltinRunner::point_on_curve(&x, &y, &alpha, &beta));
    }

    #[test]
    fn point_is_not_on_curve_b() {
        let x = felt_hex!("0x6f0a1ddaeb88837dcc8ac9a48f894deed706bc3e8998e63535e2c91a8d87fd9");
        let y = felt_hex!("0x4afa52a9ef8c023d33ea3865fb4e0e49abfc50dd50ccea867539b526d474c39");
        let alpha = Felt252::ONE;
        let beta = felt_hex!("0x6f21413efbe40de150e596d72f7a8c5609ad26c15c915c1f4cdfcb99cee9e89");
        assert!(!EcOpBuiltinRunner::point_on_curve(&x, &y, &alpha, &beta));
    }

    #[test]
    fn compute_ec_op_impl_valid_a() {
        let partial_sum = (
            felt_hex!("0x6f0a1ddaf19c44781c8946db396f494a10ffab183c2d8cf6c4cd321a8d87fd9"),
            felt_hex!("0x4afa52a9ef8c023d3385fddb6e1d78d57b0693b9b02d45d0f939b526d474c39"),
        );
        let doubled_point = (
            felt_hex!("0x1ef15c18599971b7beced415a40f0c7deacfd9b0d1819e03d723d8bc943cfca"),
            felt_hex!("0x5668060aa49730b7be4801df46ec62de53ecd11abe43a32873000c36e8dc1f"),
        );
        let m = Felt252::from(34);
        let height = 256;
        let result = EcOpBuiltinRunner::ec_op_impl(partial_sum, doubled_point, &m, height);
        assert_eq!(
            result,
            Ok((
                felt_str!(
                    "1977874238339000383330315148209250828062304908491266318460063803060754089297"
                ),
                felt_str!(
                    "2969386888251099938335087541720168257053975603483053253007176033556822156706"
                )
            ))
        );
    }

    #[test]
    fn compute_ec_op_impl_valid_b() {
        let partial_sum = (
            felt_hex!("0x68caa9509b7c2e90b4d92661cbf7c465471c1e8598c5f989691eef6653e0f38"),
            felt_hex!("0x79a8673f498531002fc549e06ff2010ffc0c191cceb7da5532acb95cdcb591"),
        );
        let doubled_point = (
            felt_hex!("0x1ef15c18599971b7beced415a40f0c7deacfd9b0d1819e03d723d8bc943cfca"),
            felt_hex!("0x5668060aa49730b7be4801df46ec62de53ecd11abe43a32873000c36e8dc1f"),
        );
        let m = Felt252::from(34);
        let height = 256;
        let result = EcOpBuiltinRunner::ec_op_impl(partial_sum, doubled_point, &m, height);
        assert_eq!(
            result,
            Ok((
                felt_str!(
                    "2778063437308421278851140253538604815869848682781135193774472480292420096757"
                ),
                felt_str!(
                    "3598390311618116577316045819420613574162151407434885460365915347732568210029"
                )
            ))
        );
    }

    #[test]
    fn compute_ec_op_invalid_same_x_coordinate() {
        let partial_sum = (
            felt_hex!("0x6f0a1ddaf19c44781c8946db396f494a10ffab183c2d8cf6c4cd321a8d87fd9"),
            felt_hex!("0x4afa52a9ef8c023d3385fddb6e1d78d57b0693b9b02d45d0f939b526d474c39"),
        );
        let doubled_point = (
            felt_hex!("0x6f0a1ddaf19c44781c8946db396f494a10ffab183c2d8cf6c4cd321a8d87fd9"),
            felt_hex!("0x4afa52a9ef8c023d3385fddb6e1d78d57b0693b9b02d45d0f939b526d474c39"),
        );
        let m = Felt252::from(34);
        let height = 256;
        let result = EcOpBuiltinRunner::ec_op_impl(partial_sum, doubled_point, &m, height);
        assert_eq!(
            result,
            Err(RunnerError::EcOpSameXCoordinate(
                EcOpBuiltinRunner::format_ec_op_error(
                    ProjectivePoint::from_affine(partial_sum.0, partial_sum.1).unwrap(),
                    m.to_biguint(),
                    ProjectivePoint::from_affine(doubled_point.0, doubled_point.1).unwrap(),
                )
                .into_boxed_str()
            ))
        );
    }

    #[test]
    /* Data taken from this program execution:
       %builtins output ec_op
       from starkware.cairo.common.cairo_builtins import EcOpBuiltin
       from starkware.cairo.common.serialize import serialize_word
       from starkware.cairo.common.ec_point import EcPoint
       from starkware.cairo.common.ec import ec_op

       func main{output_ptr: felt*, ec_op_ptr: EcOpBuiltin*}():
           let x: EcPoint = EcPoint(2089986280348253421170679821480865132823066470938446095505822317253594081284, 1713931329540660377023406109199410414810705867260802078187082345529207694986)

           let y: EcPoint = EcPoint(874739451078007766457464989774322083649278607533249481151382481072868806602,152666792071518830868575557812948353041420400780739481342941381225525861407)
           let z: EcPoint = ec_op(x,34, y)
           serialize_word(z.x)
           return()
           end
    */
    fn deduce_memory_cell_ec_op_for_preset_memory_valid() {
        let memory = memory![
            (
                (3, 0),
                (
                    "0x68caa9509b7c2e90b4d92661cbf7c465471c1e8598c5f989691eef6653e0f38",
                    16
                )
            ),
            (
                (3, 1),
                (
                    "0x79a8673f498531002fc549e06ff2010ffc0c191cceb7da5532acb95cdcb591",
                    16
                )
            ),
            (
                (3, 2),
                (
                    "0x1ef15c18599971b7beced415a40f0c7deacfd9b0d1819e03d723d8bc943cfca",
                    16
                )
            ),
            (
                (3, 3),
                (
                    "0x5668060aa49730b7be4801df46ec62de53ecd11abe43a32873000c36e8dc1f",
                    16
                )
            ),
            ((3, 4), 34),
            (
                (3, 5),
                (
                    "2778063437308421278851140253538604815869848682781135193774472480292420096757",
                    10
                )
            )
        ];
        let builtin = EcOpBuiltinRunner::new(Some(256), true);

        let result = builtin.deduce_memory_cell(Relocatable::from((3, 6)), &memory);
        assert_eq!(
            result,
            Ok(Some(MaybeRelocatable::from(felt_str!(
                "3598390311618116577316045819420613574162151407434885460365915347732568210029"
            ))))
        );
    }

    #[test]
    fn deduce_memory_cell_ec_op_for_preset_memory_unfilled_input_cells() {
        let memory = memory![
            (
                (3, 1),
                (
                    "0x79a8673f498531002fc549e06ff2010ffc0c191cceb7da5532acb95cdcb591",
                    16
                )
            ),
            (
                (3, 2),
                (
                    "0x1ef15c18599971b7beced415a40f0c7deacfd9b0d1819e03d723d8bc943cfca",
                    16
                )
            ),
            (
                (3, 3),
                (
                    "0x5668060aa49730b7be4801df46ec62de53ecd11abe43a32873000c36e8dc1f",
                    16
                )
            ),
            ((3, 4), 34),
            (
                (3, 5),
                (
                    "2778063437308421278851140253538604815869848682781135193774472480292420096757",
                    10
                )
            )
        ];

        let builtin = EcOpBuiltinRunner::new(Some(256), true);
        let result = builtin.deduce_memory_cell(Relocatable::from((3, 6)), &memory);
        assert_eq!(result, Ok(None));
    }

    #[test]
    fn deduce_memory_cell_ec_op_for_preset_memory_addr_not_an_output_cell() {
        let memory = memory![
            (
                (3, 0),
                (
                    "0x68caa9509b7c2e90b4d92661cbf7c465471c1e8598c5f989691eef6653e0f38",
                    16
                )
            ),
            (
                (3, 1),
                (
                    "0x79a8673f498531002fc549e06ff2010ffc0c191cceb7da5532acb95cdcb591",
                    16
                )
            ),
            (
                (3, 2),
                (
                    "0x1ef15c18599971b7beced415a40f0c7deacfd9b0d1819e03d723d8bc943cfca",
                    16
                )
            ),
            (
                (3, 3),
                (
                    "0x5668060aa49730b7be4801df46ec62de53ecd11abe43a32873000c36e8dc1f",
                    16
                )
            ),
            ((3, 4), 34),
            (
                (3, 5),
                (
                    "2778063437308421278851140253538604815869848682781135193774472480292420096757",
                    10
                )
            )
        ];
        let builtin = EcOpBuiltinRunner::new(Some(256), true);

        let result = builtin.deduce_memory_cell(Relocatable::from((3, 3)), &memory);
        assert_eq!(result, Ok(None));
    }

    #[test]
    fn deduce_memory_cell_ec_op_for_preset_memory_non_integer_input() {
        let memory = memory![
            (
                (3, 0),
                (
                    "0x68caa9509b7c2e90b4d92661cbf7c465471c1e8598c5f989691eef6653e0f38",
                    16
                )
            ),
            (
                (3, 1),
                (
                    "0x79a8673f498531002fc549e06ff2010ffc0c191cceb7da5532acb95cdcb591",
                    16
                )
            ),
            (
                (3, 2),
                (
                    "0x1ef15c18599971b7beced415a40f0c7deacfd9b0d1819e03d723d8bc943cfca",
                    16
                )
            ),
            ((3, 3), (1, 2)),
            ((3, 4), 34),
            (
                (3, 5),
                (
                    "2778063437308421278851140253538604815869848682781135193774472480292420096757",
                    10
                )
            )
        ];
        let builtin = EcOpBuiltinRunner::new(Some(256), true);

        assert_eq!(
            builtin.deduce_memory_cell(Relocatable::from((3, 6)), &memory),
            Err(RunnerError::Memory(MemoryError::ExpectedInteger(Box::new(
                Relocatable::from((3, 3))
            ))))
        );
    }

    #[test]
    fn get_used_cells_missing_segment_used_sizes() {
        let builtin = BuiltinRunner::EcOp(EcOpBuiltinRunner::new(Some(256), true));
        let vm = vm!();

        assert_eq!(
            builtin.get_used_cells(&vm.segments),
            Err(MemoryError::MissingSegmentUsedSizes)
        );
    }

    #[test]
    fn get_used_cells_empty() {
        let builtin = BuiltinRunner::EcOp(EcOpBuiltinRunner::new(Some(256), true));
        let mut vm = vm!();

        vm.segments.segment_used_sizes = Some(vec![0]);
        assert_eq!(builtin.get_used_cells(&vm.segments), Ok(0));
    }

    #[test]
    fn get_used_cells() {
        let builtin = BuiltinRunner::EcOp(EcOpBuiltinRunner::new(Some(256), true));
        let mut vm = vm!();

        vm.segments.segment_used_sizes = Some(vec![4]);
        assert_eq!(builtin.get_used_cells(&vm.segments), Ok(4));
    }

    #[test]
    fn initial_stackincluded_test() {
        let ec_op_builtin: BuiltinRunner = EcOpBuiltinRunner::new(Some(256), true).into();
        assert_eq!(ec_op_builtin.initial_stack(), vec![mayberelocatable!(0, 0)])
    }

    #[test]
    fn initial_stack_notincluded_test() {
        let ec_op_builtin = EcOpBuiltinRunner::new(Some(256), false);
        assert_eq!(ec_op_builtin.initial_stack(), Vec::new())
    }

    #[test]
    fn catch_point_same_x() {
        let program =
            include_bytes!("../../../../../cairo_programs/bad_programs/ec_op_same_x.json");
        let cairo_run_config = crate::cairo_run::Cairo0RunConfig {
            layout: LayoutName::all_cairo,
            ..crate::cairo_run::Cairo0RunConfig::default()
        };
        let result = crate::cairo_run::cairo_run(
            program,
            &cairo_run_config,
            &mut BuiltinHintProcessor::new_empty(),
        );
        assert!(result.is_err());
        // We need to check this way because CairoRunError doens't implement PartialEq
        match result {
            Err(CairoRunError::VirtualMachine(VirtualMachineError::RunnerError(
                RunnerError::EcOpSameXCoordinate(_),
            ))) => {}
            Err(_) => panic!("Wrong error returned, expected RunnerError::EcOpSameXCoordinate"),
            Ok(_) => panic!("Expected run to fail"),
        }
    }

    #[test]
    fn catch_point_not_in_curve() {
        let program =
            include_bytes!("../../../../../cairo_programs/bad_programs/ec_op_not_in_curve.json");
        let cairo_run_config = crate::cairo_run::Cairo0RunConfig {
            layout: LayoutName::all_cairo,
            ..crate::cairo_run::Cairo0RunConfig::default()
        };
        let result = crate::cairo_run::cairo_run(
            program,
            &cairo_run_config,
            &mut BuiltinHintProcessor::new_empty(),
        );
        assert!(result.is_err());

        // We need to check this way because CairoRunError doens't implement PartialEq
        match result {
            Err(CairoRunError::VirtualMachine(VirtualMachineError::RunnerError(
                RunnerError::PointNotOnCurve(_),
            ))) => {}
            Err(_) => panic!("Wrong error returned, expected RunnerError::EcOpSameXCoordinate"),
            Ok(_) => panic!("Expected run to fail"),
        }
    }

    #[test]
    fn get_air_private_input() {
        let builtin: BuiltinRunner = EcOpBuiltinRunner::new(Some(256), true).into();

        let segments = segments![
            ((0, 0), 0),
            ((0, 1), 1),
            ((0, 2), 2),
            ((0, 3), 3),
            ((0, 4), 4)
        ];
        assert_eq!(
            builtin.air_private_input(&segments),
            (vec![PrivateInput::EcOp(PrivateInputEcOp {
                index: 0,
                p_x: 0.into(),
                p_y: 1.into(),
                m: 4.into(),
                q_x: 2.into(),
                q_y: 3.into(),
            })])
        );
    }
}
