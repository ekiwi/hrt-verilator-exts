use crate::ops::{Comparison, ShiftOperation};
use parser_verilator::{
    ast::{
        AssignmentKind, AssignmentTarget, BinaryOperator, DataType, Design, Direction, Domain,
        Edge, Expression, ExpressionKind, PropertyKind, SignalDomain, SourceInfo, Statement,
        StatementKind, UnaryOperator, Variable, VariableId, VariableKind, collect::CollectAccesses,
        sequential,
    },
    document::AstDocument,
};
use patronus::expr::ExprRef;
use patronus::system::TransitionSystem;
use std::task::Context;
use std::{
    collections::{BTreeMap, BTreeSet},
    mem,
};

pub use crate::error::ConvertError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedSignal {
    pub name: String,
    pub expr: ExprRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedProperty {
    pub name: String,
    pub expr: ExprRef,
}

#[derive(Clone)]
pub struct NamedFsm {
    pub sys: TransitionSystem,
    pub clock: SignalDomain,
    pub reset: Option<SignalDomain>,
    pub inputs: Vec<NamedSignal>,
    pub registers: Vec<NamedSignal>,
    pub outputs: Vec<NamedSignal>,
    pub assertions: Vec<NamedProperty>,
    pub assumptions: Vec<NamedProperty>,
    pub covers: Vec<NamedProperty>,
}

impl NamedFsm {
    pub fn initialize_registers_to_zero(&mut self) {
        // for latch in self.fsm.get_latches() {
        //     self.fsm
        //         .get_latch_mut(latch.output.index())
        //         .unwrap()
        //         .reset_value = Some(false);
        // }
        todo!()
    }
}

type Environment = BTreeMap<VariableId, Vec<ExprRef>>;
type PropertyGroups = (Vec<NamedProperty>, Vec<NamedProperty>, Vec<NamedProperty>);

struct Converter<'a> {
    design: &'a Design,
    clock: SignalDomain,
    reset: Option<SignalDomain>,
    sys: TransitionSystem,
    pending: Environment,
}

impl TryFrom<&Design> for NamedFsm {
    type Error = ConvertError;

    fn try_from(design: &Design) -> Result<Self, Self::Error> {
        let (clock, reset) = select_domains(design, None, None)?;
        Converter {
            design,
            clock,
            reset,
            sys: TransitionSystem::new("todo".into()),
            pending: Environment::new(),
        }
        .convert()
    }
}

impl NamedFsm {
    pub fn from_design(
        design: &Design,
        clock: Domain,
        reset: Option<Domain>,
    ) -> Result<Self, ConvertError> {
        let (clock, reset) = select_domains(design, Some(&clock), reset.as_ref())?;
        Converter {
            design,
            clock,
            reset,
            sys: TransitionSystem::new("todo".into()),
            pending: Environment::new(),
        }
        .convert()
    }

    pub fn from_document(
        document: &AstDocument,
        clock: Domain,
        reset: Option<Domain>,
    ) -> Result<Self, ConvertError> {
        let design = Design::try_from(document)?;
        Self::from_design(&design, clock, reset)
    }
}

impl TryFrom<&AstDocument> for NamedFsm {
    type Error = ConvertError;

    fn try_from(document: &AstDocument) -> Result<Self, Self::Error> {
        let design = Design::try_from(document)?;
        Self::try_from(&design)
    }
}

fn select_domains(
    design: &Design,
    clock: Option<&Domain>,
    reset: Option<&Domain>,
) -> Result<(SignalDomain, Option<SignalDomain>), ConvertError> {
    if clock
        .zip(reset)
        .is_some_and(|(clock, reset)| clock.name == reset.name)
    {
        return Err(ConvertError::message(
            "clock and reset must name different signals",
        ));
    }

    let clock_domain = if let Some(clock) = clock {
        (&design.sensitivity_domains)
            .into_iter()
            .find(|domain| domain.domain == *clock)
            .cloned()
            .ok_or_else(|| {
                ConvertError::message(format!("expected clock {clock:?} but it was not found",))
            })?
    } else {
        if design.sensitivity_domains.len() != 1 {
            return Err(ConvertError::message(format!(
                "expected one clock domain, found {}",
                design.sensitivity_domains.len()
            )));
        }
        design.sensitivity_domains[0].clone()
    };

    let reset_domain = reset
        .map(|reset| resolve_reset(design, reset))
        .transpose()?;
    for domain in &design.sensitivity_domains {
        if *domain == clock_domain || reset_domain.as_ref().is_some_and(|reset| *domain == *reset) {
            continue;
        }
        return Err(ConvertError::message(format!(
            "encountered {domain:?} but expected clock {clock_domain:?}",
        )));
    }
    Ok((clock_domain, reset_domain))
}

fn resolve_reset(design: &Design, reset: &Domain) -> Result<SignalDomain, ConvertError> {
    let matches = (&design.variables)
        .into_iter()
        .enumerate()
        .filter(|(_, variable)| variable.display_name() == reset.name)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(ConvertError::message(format!(
            "expected exactly one reset signal named {}, found {}",
            reset.name,
            matches.len()
        )));
    }
    let (index, variable) = matches[0];
    if variable.direction != Direction::Input {
        return Err(ConvertError::message(format!(
            "reset signal {} must be an input",
            reset.name
        )));
    }
    if design.data_type(variable.dtype).width != 1 {
        return Err(ConvertError::message(format!(
            "reset signal {} must be one bit wide",
            reset.name
        )));
    }
    Ok(SignalDomain {
        variable: VariableId(index),
        domain: reset.clone(),
    })
}

fn sanitize_symbol(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character == '\n' || character == '\r' {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn signal_bit_names(signal: &NamedSignal) -> Vec<String> {
    signal_bit_names_as(signal, &signal.name)
}

fn signal_bit_names_as(signal: &NamedSignal, name: &str) -> Vec<String> {
    todo!()
    // let mut names = Vec::new();
    // if signal.bits.len() == 1 {
    //     names.push(name.to_string());
    // } else {
    //     for bit in &signal.bits {
    //         names.push(format!("{}[{}]", name, bit.index));
    //     }
    // }
    // names
}

fn label_variables(sys: &mut TransitionSystem, signal: &NamedSignal) {
    label_variables_as(sys, signal, &signal.name);
}

fn label_variables_as(sys: &mut TransitionSystem, signal: &NamedSignal, name: &str) {
    // for (bit, name) in (&signal.bits)
    //     .into_iter()
    //     .zip(signal_bit_names_as(signal, name))
    // {
    //     let variable = bit.value.unwrap_variable();
    //     *fsm.get_variable_label_mut(variable.index()) = Some(sanitize_symbol(&name));
    // }
    todo!()
}

impl Converter<'_> {
    fn convert(mut self) -> Result<NamedFsm, ConvertError> {
        todo!()

        // if let Some(initial) = (&self.design.initial)
        //     .into_iter()
        //     .find(|statement| !is_formal_static_initializer(self.design, statement))
        // {
        //     return Err(ConvertError::source(
        //         &initial.source,
        //         "initial blocks are not supported by AIGER conversion",
        //     ));
        // }
        // let all_statements = (&self.design.combinational)
        //     .into_iter()
        //     .chain(&self.design.sequential)
        //     .cloned()
        //     .collect::<Vec<_>>();
        // let mut reads = BTreeSet::new();
        // let mut writes = BTreeSet::new();
        // all_statements
        //     .as_slice()
        //     .collect_accesses(&mut reads, &mut writes);
        // loop {
        //     let before = reads.len();
        //     for (index, variable) in (&self.design.variables).into_iter().enumerate() {
        //         let id = VariableId(index);
        //         if reads.contains(&id)
        //             && let Some(sampled) = &variable.sampled_value
        //         {
        //             sampled.collect_reads(&mut reads);
        //         }
        //     }
        //     if reads.len() == before {
        //         break;
        //     }
        // }
        //
        // let sequential = sequential::analyze(self.design).map_err(ConvertError::message)?;
        // let mut register_ids = sequential.registers.clone();
        // // Preserve the existing unpacked-array storage model for procedural
        // // combinational element writes, which read the untouched elements.
        // register_ids.extend(
        //     (&self.design.variables)
        //         .into_iter()
        //         .enumerate()
        //         .filter(|(index, variable)| {
        //             self.design.data_type(variable.dtype).unpacked.is_some()
        //                 && reads.contains(&VariableId(*index))
        //                 && writes.contains(&VariableId(*index))
        //         })
        //         .map(|(index, _)| VariableId(index)),
        // );
        //
        // let input_ids = (&self.design.variables)
        //     .into_iter()
        //     .enumerate()
        //     .filter_map(|(index, variable)| {
        //         let id = VariableId(index);
        //         let primary_input = variable.direction == Direction::Input;
        //         let undriven_read = reads.contains(&id)
        //             && !writes.contains(&id)
        //             && !register_ids.contains(&id)
        //             && variable.sampled_value.is_none()
        //             && !variable.internal;
        //         ((primary_input || undriven_read) && id != self.clock.variable).then_some(id)
        //     })
        //     .collect::<BTreeSet<_>>();
        //
        // let mut environment = Environment::new();
        // let mut inputs = Vec::new();
        // for id in (0..self.design.variables.len())
        //     .map(VariableId)
        //     .filter(|id| input_ids.contains(id))
        // {
        //     let variable = self.design.variable(id);
        //     let values = (0..self.design.data_type(variable.dtype).width)
        //         .map(|_| ExprRef::from(self.fsm.add_variable_input()))
        //         .collect::<Vec<_>>();
        //     environment.insert(id, values.clone());
        //     let signal = named_signal(self.design, variable, &values);
        //     label_variables(&mut self.fsm, &signal);
        //     inputs.push(signal);
        // }
        //
        // let mut register_variables = BTreeMap::new();
        // let mut registers = Vec::new();
        // for id in (0..self.design.variables.len())
        //     .map(VariableId)
        //     .filter(|id| register_ids.contains(id))
        // {
        //     let variable = self.design.variable(id);
        //     let values = (0..self.design.data_type(variable.dtype).width)
        //         .map(|_| ExprRef::from(self.fsm.add_variable()))
        //         .collect::<Vec<_>>();
        //     environment.insert(id, values.clone());
        //     register_variables.insert(id, values.clone());
        //     let signal = named_signal(self.design, variable, &values);
        //     label_variables(&mut self.fsm, &signal);
        //     registers.push(signal);
        // }
        //
        // self.execute_combinational(&self.design.combinational, &mut environment)?;
        // // Freeze sampled expressions before any clocked blocking assignments run.
        // let before_edge = environment.clone();
        // for (index, variable) in (&self.design.variables).into_iter().enumerate() {
        //     if let Some(sampled) = &variable.sampled_value {
        //         let value = self.expression(sampled, &before_edge)?;
        //         environment.insert(VariableId(index), value);
        //     }
        // }
        // for id in &sequential.nonblocking {
        //     self.pending.insert(*id, environment[id].clone());
        // }
        // self.execute_all(&self.design.sequential, &mut environment)?;
        // for (register, outputs) in &register_variables {
        //     let next = self
        //         .pending
        //         .get(register)
        //         .or_else(|| environment.get(register))
        //         .cloned()
        //         .ok_or_else(|| ConvertError::message("register has no next value"))?;
        //     self.add_latches(*register, outputs, &next)?;
        // }
        //
        // // Outputs describe the current FSM state; blocking procedural writes
        // // above compute next-state values, just like deferred NBA writes.
        // for id in &sequential.registers {
        //     environment.insert(*id, register_variables[id].clone());
        // }
        // let mut outputs = Vec::new();
        // for (index, variable) in (&self.design.variables).into_iter().enumerate() {
        //     if variable.direction != Direction::Output {
        //         continue;
        //     }
        //     let id = VariableId(index);
        //     let values = environment.get(&id).ok_or_else(|| {
        //         ConvertError::source(&variable.source, "output has no symbolic value")
        //     })?;
        //     let signal = named_signal(self.design, variable, values);
        //     for (&value, name) in values.into_iter().zip(signal_bit_names(&signal)) {
        //         let output = self.fsm.add_output(value);
        //         *self.fsm.get_output_label_mut(output) = Some(sanitize_symbol(&name));
        //     }
        //     outputs.push(signal);
        // }
        //
        // let (assertions, assumptions, covers) = self.add_properties(&environment)?;
        // let mut model = NamedFsm {
        //     fsm: self.fsm,
        //     clock: self.clock,
        //     reset: self.reset.clone(),
        //     inputs,
        //     registers,
        //     outputs,
        //     assertions,
        //     assumptions,
        //     covers,
        // };
        // reorder_model(&mut model);
        // for (register, signal) in register_ids.into_iter().zip(&model.registers) {
        //     label_variables_as(&mut model.fsm, signal, &self.design.variable(register).name);
        // }
        // if let Some(reset) = &self.reset {
        //     apply_reset(&mut model, reset)?;
        // }
        // model.fsm.verify(VerifyOrdering::Verify);
        // Ok(model)
    }
    //
    // fn add_latches(
    //     &mut self,
    //     register: VariableId,
    //     outputs: &[ExprRef],
    //     next: &[ExprRef],
    // ) -> Result<(), ConvertError> {
    //     if next.len() != outputs.len() {
    //         return Err(ConvertError::message(format!(
    //             "register {} next-state width mismatch",
    //             self.design.variable(register).display_name()
    //         )));
    //     }
    //     let history_value = formal_history_initial_value(self.design.variable(register));
    //     for (&output, &input) in outputs.into_iter().zip(next) {
    //         self.fsm
    //             .add_latch(input, output.unwrap_variable(), history_value);
    //     }
    //     Ok(())
    // }
    //
    // fn execute_combinational(
    //     &mut self,
    //     statements: &[Statement],
    //     environment: &mut Environment,
    // ) -> Result<(), ConvertError> {
    //     let mut pending = statements.into_iter().collect::<Vec<_>>();
    //     while !pending.is_empty() {
    //         let Some(index) = (&pending).into_iter().position(|statement| {
    //             let mut reads = BTreeSet::new();
    //             statement.collect_reads(&mut reads);
    //             reads.into_iter().all(|id| environment.contains_key(&id))
    //         }) else {
    //             return Err(ConvertError::message(
    //                 "combinational logic has a cycle or unresolved input",
    //             ));
    //         };
    //         let statement = pending.remove(index);
    //         self.execute(statement, environment).map_err(|error| {
    //             ConvertError::source(
    //                 &statement.source,
    //                 format!("combinational evaluation failed: {error}"),
    //             )
    //         })?;
    //     }
    //     Ok(())
    // }
    //
    // fn execute_all(
    //     &mut self,
    //     statements: &[Statement],
    //     environment: &mut Environment,
    // ) -> Result<(), ConvertError> {
    //     for statement in statements {
    //         self.execute(statement, environment)?;
    //     }
    //     Ok(())
    // }
    //
    // fn execute(
    //     &mut self,
    //     statement: &Statement,
    //     environment: &mut Environment,
    // ) -> Result<(), ConvertError> {
    //     match &statement.kind {
    //         StatementKind::Block { statements, .. } => self.execute_all(statements, environment),
    //         StatementKind::Assignment {
    //             kind,
    //             target,
    //             value,
    //         } => {
    //             let value = self.expression(value, environment)?;
    //             if *kind == AssignmentKind::Nonblocking {
    //                 let mut pending = mem::take(&mut self.pending);
    //                 let result = self.assign(target, value, &mut pending, environment);
    //                 self.pending = pending;
    //                 result
    //             } else {
    //                 let evaluation = environment.clone();
    //                 self.assign(target, value, environment, &evaluation)
    //             }
    //         }
    //         StatementKind::If {
    //             condition,
    //             then_statements,
    //             else_statements,
    //         } => {
    //             let condition_value = self.expression(condition, environment)?;
    //             let condition = self.truthy(&condition_value);
    //             let before = environment.clone();
    //             let pending_before = self.pending.clone();
    //             let mut then_environment = before.clone();
    //             self.execute_all(then_statements, &mut then_environment)?;
    //             let then_pending = mem::replace(&mut self.pending, pending_before);
    //             let mut else_environment = before.clone();
    //             self.execute_all(else_statements, &mut else_environment)?;
    //             let else_pending = mem::take(&mut self.pending);
    //             self.pending = self.merge_environments(
    //                 &statement.source,
    //                 condition,
    //                 &then_pending,
    //                 &else_pending,
    //                 &Environment::new(),
    //             )?;
    //             *environment = self.merge_environments(
    //                 &statement.source,
    //                 condition,
    //                 &then_environment,
    //                 &else_environment,
    //                 &before,
    //             )?;
    //             Ok(())
    //         }
    //     }
    // }
    //
    // fn merge_environments(
    //     &mut self,
    //     source: &SourceInfo,
    //     condition: ExprRef,
    //     then_environment: &Environment,
    //     else_environment: &Environment,
    //     before: &Environment,
    // ) -> Result<Environment, ConvertError> {
    //     let mut environment = Environment::new();
    //     let keys = then_environment
    //         .keys()
    //         .chain(else_environment.keys())
    //         .copied()
    //         .collect::<BTreeSet<_>>();
    //     for key in keys {
    //         let width = self
    //             .design
    //             .variables
    //             .get(key.0)
    //             .map_or(0, |variable| self.design.data_type(variable.dtype).width);
    //         let default = vec![ExprRef::Constant(false); width];
    //         let then_value = then_environment
    //             .get(&key)
    //             .or_else(|| before.get(&key))
    //             .unwrap_or(&default);
    //         let else_value = else_environment
    //             .get(&key)
    //             .or_else(|| before.get(&key))
    //             .unwrap_or(&default);
    //         if then_value == else_value {
    //             environment.insert(key, then_value.clone());
    //         } else {
    //             if then_value.len() != else_value.len() {
    //                 return Err(ConvertError::source(source, "IF branch width mismatch"));
    //             }
    //             environment.insert(
    //                 key,
    //                 FsmOps::create_mux(&mut self.fsm, else_value, then_value, condition),
    //             );
    //         }
    //     }
    //     Ok(environment)
    // }
    //
    // fn assign(
    //     &mut self,
    //     target: &AssignmentTarget,
    //     value: Vec<ExprRef>,
    //     environment: &mut Environment,
    //     evaluation: &Environment,
    // ) -> Result<(), ConvertError> {
    //     match target {
    //         AssignmentTarget::Variable { variable, .. } => {
    //             let width = self
    //                 .design
    //                 .data_type(self.design.variable(*variable).dtype)
    //                 .width;
    //             environment.insert(
    //                 *variable,
    //                 resize(value, width, false, ExprRef::Constant(false)),
    //             );
    //             Ok(())
    //         }
    //         AssignmentTarget::Select {
    //             target,
    //             offset,
    //             width,
    //             ..
    //         } => {
    //             let target_value = self.read_target(target, environment, evaluation)?;
    //             if value.len() != *width || *width > target_value.len() {
    //                 return Err(ConvertError::source(
    //                     &offset.source,
    //                     "SEL assignment width mismatch",
    //                 ));
    //             }
    //             if let ExpressionKind::Constant(literal) = &offset.kind {
    //                 let offset = usize::try_from(&literal.value).unwrap();
    //                 let mut result = target_value;
    //                 result[offset..offset + width].copy_from_slice(&value);
    //                 return self.assign(target, result, environment, evaluation);
    //             }
    //             let offset_value = self.expression(offset, evaluation)?;
    //             let mut result = target_value.clone();
    //             for candidate_offset in 0..=target_value.len() - width {
    //                 let candidate_value = usize_values(candidate_offset, offset_value.len());
    //                 let selected = self.equals_constant_without_or(&offset_value, &candidate_value);
    //                 let mut candidate = target_value.clone();
    //                 candidate[candidate_offset..candidate_offset + width].copy_from_slice(&value);
    //                 result = self.mux_without_or(&result, &candidate, selected);
    //             }
    //             self.assign(target, result, environment, evaluation)
    //         }
    //         AssignmentTarget::ArrayElement {
    //             array,
    //             index,
    //             dtype,
    //         } => {
    //             let layout = self.design.data_type(*dtype).unpacked.as_ref().unwrap();
    //             if value.len() != layout.element_width {
    //                 return Err(ConvertError::source(
    //                     &index.source,
    //                     "ARRAYSEL assignment width mismatch",
    //                 ));
    //             }
    //             let index_value = self.expression(index, evaluation)?;
    //             let current = self.read_target(array, environment, evaluation)?;
    //             let mut result = current.clone();
    //             for (offset, declared_index) in layout.indices.into_iter().enumerate() {
    //                 let candidate =
    //                     usize_values(usize::try_from(declared_index).unwrap(), index_value.len());
    //                 let selected = self.equals_constant_without_or(&index_value, &candidate);
    //                 let mut updated = current.clone();
    //                 let start = offset * layout.element_width;
    //                 updated[start..start + layout.element_width].copy_from_slice(&value);
    //                 result = self.mux_without_or(&result, &updated, selected);
    //             }
    //             self.assign(array, result, environment, evaluation)
    //         }
    //     }
    // }
    //
    // fn read_target(
    //     &mut self,
    //     target: &AssignmentTarget,
    //     environment: &Environment,
    //     evaluation: &Environment,
    // ) -> Result<Vec<ExprRef>, ConvertError> {
    //     match target {
    //         AssignmentTarget::Variable { variable, .. } => {
    //             environment.get(variable).cloned().ok_or_else(|| {
    //                 ConvertError::message(format!(
    //                     "assignment target {} is unresolved",
    //                     self.design.variable(*variable).display_name()
    //                 ))
    //             })
    //         }
    //         AssignmentTarget::Select { offset, width, .. } => {
    //             let AssignmentTarget::Select { target, .. } = target else {
    //                 unreachable!()
    //             };
    //             let source = self.read_target(target, environment, evaluation)?;
    //             self.select_value(source, offset, *width, evaluation)
    //         }
    //         AssignmentTarget::ArrayElement {
    //             array,
    //             index,
    //             dtype,
    //         } => {
    //             let source = self.read_target(array, environment, evaluation)?;
    //             self.array_select_value(source, index, self.design.data_type(*dtype), evaluation)
    //         }
    //     }
    // }
    //
    // fn expression(
    //     &mut self,
    //     expression: &Expression,
    //     environment: &Environment,
    // ) -> Result<Vec<ExprRef>, ConvertError> {
    //     let width = self.design.data_type(expression.dtype).width;
    //     match &expression.kind {
    //         ExpressionKind::Constant(literal) => Ok((0..width)
    //             .map(|bit| ExprRef::Constant(literal.value.bit(bit as u64)))
    //             .collect()),
    //         ExpressionKind::Variable { variable, .. } => {
    //             if let Some(value) = environment.get(variable) {
    //                 return Ok(value.clone());
    //             }
    //             if let Some(sampled) = &self.design.variable(*variable).sampled_value {
    //                 return self.expression(sampled, environment);
    //             }
    //             Err(ConvertError::source(
    //                 &expression.source,
    //                 format!(
    //                     "unresolved symbolic variable {}",
    //                     self.design.variable(*variable).display_name()
    //                 ),
    //             ))
    //         }
    //         ExpressionKind::Unary { operator, operand } => {
    //             self.unary_expression(expression, *operator, operand, environment)
    //         }
    //         ExpressionKind::Binary { operator, lhs, rhs } => {
    //             self.binary_expression(expression, *operator, lhs, rhs, environment)
    //         }
    //         ExpressionKind::Conditional {
    //             condition,
    //             then_value,
    //             else_value,
    //         } => {
    //             let condition_value = self.expression(condition, environment)?;
    //             let select = self.truthy(&condition_value);
    //             let then_value = resize(
    //                 self.expression(then_value, environment)?,
    //                 width,
    //                 false,
    //                 ExprRef::Constant(false),
    //             );
    //             let else_value = resize(
    //                 self.expression(else_value, environment)?,
    //                 width,
    //                 false,
    //                 ExprRef::Constant(false),
    //             );
    //             Ok(FsmOps::create_mux(
    //                 &mut self.fsm,
    //                 &else_value,
    //                 &then_value,
    //                 select,
    //             ))
    //         }
    //         ExpressionKind::Replicate { source, count, .. } => {
    //             let source = self.expression(source, environment)?;
    //             let mut result = Vec::with_capacity(width);
    //             for _ in 0..*count {
    //                 result.extend(&source);
    //             }
    //             Ok(result)
    //         }
    //         ExpressionKind::Select {
    //             value,
    //             offset,
    //             width,
    //         } => {
    //             let value = self.expression(value, environment)?;
    //             self.select_value(value, offset, *width, environment)
    //         }
    //         ExpressionKind::ArraySelect { array, index } => {
    //             let value = self.expression(array, environment)?;
    //             self.array_select_value(
    //                 value,
    //                 index,
    //                 self.design.data_type(array.dtype),
    //                 environment,
    //             )
    //         }
    //     }
    // }
    //
    // fn unary_expression(
    //     &mut self,
    //     expression: &Expression,
    //     operator: UnaryOperator,
    //     operand: &Expression,
    //     environment: &Environment,
    // ) -> Result<Vec<ExprRef>, ConvertError> {
    //     let value = self.expression(operand, environment)?;
    //     let width = self.design.data_type(expression.dtype).width;
    //     Ok(match operator {
    //         UnaryOperator::BitwiseNot => value.into_iter().map(|value| !value).collect(),
    //         UnaryOperator::Negate => {
    //             let zero = vec![ExprRef::Constant(false); value.len()];
    //             FsmOps::create_subtraction(&mut self.fsm, &zero, &value)
    //         }
    //         UnaryOperator::ReduceAnd => vec![self.fsm.add_variable_gate(GateType::And, value)],
    //         UnaryOperator::ReduceOr => vec![self.fsm.add_variable_gate(GateType::Or, value)],
    //         UnaryOperator::ReduceXor => {
    //             let mut result = ExprRef::Constant(false);
    //             for value in value {
    //                 result = FsmOps::create_xor_gate(&mut self.fsm, result, value);
    //             }
    //             vec![result]
    //         }
    //         UnaryOperator::LogicalNot => vec![!self.truthy(&value)],
    //         UnaryOperator::ZeroExtend => resize(value, width, false, ExprRef::Constant(false)),
    //         UnaryOperator::SignExtend => resize(value, width, true, ExprRef::Constant(false)),
    //     })
    // }
    //
    // fn binary_expression(
    //     &mut self,
    //     expression: &Expression,
    //     operator: BinaryOperator,
    //     lhs: &Expression,
    //     rhs: &Expression,
    //     environment: &Environment,
    // ) -> Result<Vec<ExprRef>, ConvertError> {
    //     let width = self.design.data_type(expression.dtype).width;
    //     let lhs_value = self.expression(lhs, environment)?;
    //     let rhs_value = self.expression(rhs, environment)?;
    //     Ok(match operator {
    //         BinaryOperator::BitwiseAnd | BinaryOperator::BitwiseOr | BinaryOperator::BitwiseXor => {
    //             let lhs = resize(lhs_value, width, false, ExprRef::Constant(false));
    //             let rhs = resize(rhs_value, width, false, ExprRef::Constant(false));
    //             lhs.into_iter()
    //                 .zip(rhs)
    //                 .map(|(lhs, rhs)| match operator {
    //                     BinaryOperator::BitwiseAnd => ExprRef::from(
    //                         self.fsm.add_variable_gate_binary(GateType::And, lhs, rhs),
    //                     ),
    //                     BinaryOperator::BitwiseOr => {
    //                         ExprRef::from(self.fsm.add_variable_gate_binary(GateType::Or, lhs, rhs))
    //                     }
    //                     BinaryOperator::BitwiseXor => {
    //                         FsmOps::create_xor_gate(&mut self.fsm, lhs, rhs)
    //                     }
    //                     _ => unreachable!(),
    //                 })
    //                 .collect()
    //         }
    //         BinaryOperator::Add | BinaryOperator::Subtract => {
    //             let signed = self.design.data_type(expression.dtype).signed;
    //             let lhs = resize(lhs_value, width, signed, ExprRef::Constant(false));
    //             let rhs = resize(rhs_value, width, signed, ExprRef::Constant(false));
    //             if operator == BinaryOperator::Add {
    //                 FsmOps::create_addition(&mut self.fsm, &lhs, &rhs)
    //             } else {
    //                 FsmOps::create_subtraction(&mut self.fsm, &lhs, &rhs)
    //             }
    //         }
    //         BinaryOperator::MultiplyUnsigned | BinaryOperator::MultiplySigned => {
    //             let signed = operator == BinaryOperator::MultiplySigned;
    //             let lhs = resize(lhs_value, width, signed, ExprRef::Constant(false));
    //             let rhs = resize(rhs_value, width, signed, ExprRef::Constant(false));
    //             FsmOps::create_multiplication(&mut self.fsm, &lhs, &rhs)
    //         }
    //         BinaryOperator::DivideUnsigned => {
    //             let lhs = resize(lhs_value, width, false, ExprRef::Constant(false));
    //             let rhs = resize(rhs_value, width, false, ExprRef::Constant(false));
    //             FsmOps::create_unsigned_division(&mut self.fsm, &lhs, &rhs)
    //         }
    //         BinaryOperator::DivideSigned => {
    //             let lhs = resize(lhs_value, width, true, ExprRef::Constant(false));
    //             let rhs = resize(rhs_value, width, true, ExprRef::Constant(false));
    //             FsmOps::create_signed_division(&mut self.fsm, &lhs, &rhs)
    //         }
    //         BinaryOperator::Equal
    //         | BinaryOperator::NotEqual
    //         | BinaryOperator::LessThanUnsigned
    //         | BinaryOperator::LessThanOrEqualUnsigned
    //         | BinaryOperator::GreaterThanUnsigned
    //         | BinaryOperator::GreaterThanOrEqualUnsigned
    //         | BinaryOperator::LessThanSigned
    //         | BinaryOperator::LessThanOrEqualSigned
    //         | BinaryOperator::GreaterThanSigned
    //         | BinaryOperator::GreaterThanOrEqualSigned => {
    //             let width = lhs_value.len().max(rhs_value.len());
    //             let signed = operator == BinaryOperator::LessThanSigned
    //                 || operator == BinaryOperator::LessThanOrEqualSigned
    //                 || operator == BinaryOperator::GreaterThanSigned
    //                 || operator == BinaryOperator::GreaterThanOrEqualSigned;
    //             let mut lhs = resize(lhs_value, width, signed, ExprRef::Constant(false));
    //             let mut rhs = resize(rhs_value, width, signed, ExprRef::Constant(false));
    //             if signed && width != 0 {
    //                 lhs[width - 1] = !lhs[width - 1];
    //                 rhs[width - 1] = !rhs[width - 1];
    //             }
    //             let comparison = match operator {
    //                 BinaryOperator::Equal => Comparison::Equals,
    //                 BinaryOperator::NotEqual => Comparison::NotEquals,
    //                 BinaryOperator::LessThanUnsigned | BinaryOperator::LessThanSigned => {
    //                     Comparison::LessThan
    //                 }
    //                 BinaryOperator::LessThanOrEqualUnsigned
    //                 | BinaryOperator::LessThanOrEqualSigned => Comparison::LessThanOrEqual,
    //                 BinaryOperator::GreaterThanUnsigned | BinaryOperator::GreaterThanSigned => {
    //                     Comparison::GreaterThan
    //                 }
    //                 BinaryOperator::GreaterThanOrEqualUnsigned
    //                 | BinaryOperator::GreaterThanOrEqualSigned => Comparison::GreaterThanOrEqual,
    //                 _ => unreachable!(),
    //             };
    //             vec![FsmOps::create_comparison(
    //                 &mut self.fsm,
    //                 &lhs,
    //                 &rhs,
    //                 comparison,
    //             )]
    //         }
    //         BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr => {
    //             let lhs = self.truthy(&lhs_value);
    //             let rhs = self.truthy(&rhs_value);
    //             let gate = if operator == BinaryOperator::LogicalAnd {
    //                 GateType::And
    //             } else {
    //                 GateType::Or
    //             };
    //             vec![ExprRef::from(
    //                 self.fsm.add_variable_gate_binary(gate, lhs, rhs),
    //             )]
    //         }
    //         BinaryOperator::ShiftLeft
    //         | BinaryOperator::ShiftRight
    //         | BinaryOperator::ShiftRightArithmetic => {
    //             self.shift(operator, lhs_value, rhs_value, width)
    //         }
    //         BinaryOperator::Concat => {
    //             let mut result = rhs_value;
    //             result.extend(lhs_value);
    //             result
    //         }
    //     })
    // }
    //
    // fn shift(
    //     &mut self,
    //     operator: BinaryOperator,
    //     value: Vec<ExprRef>,
    //     shift: Vec<ExprRef>,
    //     width: usize,
    // ) -> Vec<ExprRef> {
    //     let value = resize(value, width, false, ExprRef::Constant(false));
    //     let padded_width = width.next_power_of_two();
    //     let useful_shift_bits = if padded_width <= 1 {
    //         0
    //     } else {
    //         padded_width.trailing_zeros() as usize
    //     };
    //     if useful_shift_bits == 0 {
    //         return value;
    //     }
    //     let low_shift = resize(
    //         shift.clone(),
    //         useful_shift_bits,
    //         false,
    //         ExprRef::Constant(false),
    //     );
    //     let operation = match operator {
    //         BinaryOperator::ShiftLeft => ShiftOperation::ShiftLeft,
    //         BinaryOperator::ShiftRight => ShiftOperation::ShiftRight,
    //         BinaryOperator::ShiftRightArithmetic => ShiftOperation::ShiftRightArithmetic,
    //         _ => unreachable!(),
    //     };
    //     let fill = if operation == ShiftOperation::ShiftRightArithmetic {
    //         *value.last().unwrap()
    //     } else {
    //         ExprRef::Constant(false)
    //     };
    //     let padded_value = resize(value, padded_width, false, fill);
    //     let mut result =
    //         FsmOps::create_shifter(&mut self.fsm, &padded_value, &low_shift, operation);
    //     result.truncate(width);
    //     if shift.len() > useful_shift_bits {
    //         let oversized = self.truthy(&shift[useful_shift_bits..]);
    //         let saturated = vec![fill; width];
    //         result = FsmOps::create_mux(&mut self.fsm, &result, &saturated, oversized);
    //     }
    //     result
    // }
    //
    // fn select_value(
    //     &mut self,
    //     value: Vec<ExprRef>,
    //     offset: &Expression,
    //     width: usize,
    //     environment: &Environment,
    // ) -> Result<Vec<ExprRef>, ConvertError> {
    //     if let ExpressionKind::Constant(literal) = &offset.kind {
    //         let offset = usize::try_from(&literal.value).unwrap();
    //         return Ok(value[offset..offset + width].to_vec());
    //     }
    //     let offset_value = self.expression(offset, environment)?;
    //     let choice_count = value.len().checked_next_power_of_two().ok_or_else(|| {
    //         ConvertError::source(&offset.source, "dynamic selection input is too wide")
    //     })?;
    //     let useful_offset_bits = choice_count.trailing_zeros() as usize;
    //     let low_offset = resize(
    //         offset_value.clone(),
    //         useful_offset_bits,
    //         false,
    //         ExprRef::Constant(false),
    //     );
    //     let mut result = (0..width)
    //         .map(|bit| {
    //             let choices = (0..choice_count)
    //                 .map(|candidate_offset| {
    //                     value
    //                         .get(candidate_offset + bit)
    //                         .copied()
    //                         .unwrap_or(ExprRef::Constant(false))
    //                 })
    //                 .collect::<Vec<_>>();
    //             self.select_without_or(&choices, &low_offset)
    //         })
    //         .collect::<Vec<_>>();
    //     if offset_value.len() > useful_offset_bits {
    //         let oversized = self.truthy(&offset_value[useful_offset_bits..]);
    //         let zero = vec![ExprRef::Constant(false); width];
    //         result = FsmOps::create_mux(&mut self.fsm, &result, &zero, oversized);
    //     }
    //     Ok(result)
    // }
    //
    // fn array_select_value(
    //     &mut self,
    //     array: Vec<ExprRef>,
    //     index: &Expression,
    //     dtype: &DataType,
    //     environment: &Environment,
    // ) -> Result<Vec<ExprRef>, ConvertError> {
    //     let layout = dtype.unpacked.as_ref().unwrap();
    //     let index = self.expression(index, environment)?;
    //     let mut result = vec![ExprRef::Constant(false); layout.element_width];
    //     for (offset, declared_index) in layout.indices.into_iter().enumerate() {
    //         let candidate = usize_values(usize::try_from(declared_index).unwrap(), index.len());
    //         let selected = self.equals_constant_without_or(&index, &candidate);
    //         let start = offset * layout.element_width;
    //         let element = &array[start..start + layout.element_width];
    //         result = self.mux_without_or(&result, element, selected);
    //     }
    //     Ok(result)
    // }
    //
    // fn truthy(&mut self, value: &[ExprRef]) -> ExprRef {
    //     if value.len() == 1 {
    //         value[0]
    //     } else {
    //         self.fsm.add_variable_gate(GateType::Or, value.to_vec())
    //     }
    // }
    //
    // fn equals_constant_without_or(&mut self, value: &[ExprRef], constant: &[ExprRef]) -> ExprRef {
    //     let equal_bits = value
    //         .into_iter()
    //         .zip(constant)
    //         .map(|(&value, &constant)| {
    //             if constant == ExprRef::Constant(true) {
    //                 value
    //             } else {
    //                 !value
    //             }
    //         })
    //         .collect();
    //     self.fsm.add_variable_gate(GateType::And, equal_bits)
    // }
    //
    // fn mux_value_without_or(&mut self, lhs: ExprRef, rhs: ExprRef, select: ExprRef) -> ExprRef {
    //     let select_lhs = ExprRef::from(self.fsm.add_variable_gate_binary(
    //         GateType::And,
    //         lhs,
    //         !select,
    //     ));
    //     let select_rhs = ExprRef::from(self.fsm.add_variable_gate_binary(
    //         GateType::And,
    //         rhs,
    //         select,
    //     ));
    //     !ExprRef::from(
    //         self.fsm
    //             .add_variable_gate_binary(GateType::And, !select_lhs, !select_rhs),
    //     )
    // }
    //
    // fn mux_without_or(
    //     &mut self,
    //     lhs: &[ExprRef],
    //     rhs: &[ExprRef],
    //     select: ExprRef,
    // ) -> Vec<ExprRef> {
    //     lhs.into_iter()
    //         .zip(rhs)
    //         .map(|(&lhs, &rhs)| self.mux_value_without_or(lhs, rhs, select))
    //         .collect()
    // }
    //
    // fn select_without_or(&mut self, values: &[ExprRef], index: &[ExprRef]) -> ExprRef {
    //     let mut level = values.to_vec();
    //     for &select in index {
    //         let mut next = Vec::with_capacity(level.len() / 2);
    //         for pair in level.as_chunks::<2>().0 {
    //             next.push(self.mux_value_without_or(pair[0], pair[1], select));
    //         }
    //         level = next;
    //     }
    //     level[0]
    // }
    //
    // fn add_properties(
    //     &mut self,
    //     environment: &Environment,
    // ) -> Result<PropertyGroups, ConvertError> {
    //     let mut assertions = Vec::new();
    //     let mut assumptions = Vec::new();
    //     let mut covers = Vec::new();
    //     for (index, variable) in (&self.design.variables).into_iter().enumerate() {
    //         let Some(property) = &variable.property else {
    //             continue;
    //         };
    //         let values = environment.get(&VariableId(index)).ok_or_else(|| {
    //             ConvertError::source(&variable.source, "formal wire has no symbolic value")
    //         })?;
    //         if values.len() != 1 {
    //             return Err(ConvertError::source(
    //                 &variable.source,
    //                 "formal wire is not one bit",
    //             ));
    //         }
    //         let named = NamedProperty {
    //             name: property.name.clone(),
    //             value: if property.kind == PropertyKind::Assertion {
    //                 !values[0]
    //             } else {
    //                 values[0]
    //             },
    //         };
    //         match property.kind {
    //             PropertyKind::Assertion => {
    //                 let index = self.fsm.add_assert(named.value);
    //                 *self.fsm.get_assert_label_mut(index) = Some(sanitize_symbol(&named.name));
    //                 assertions.push(named);
    //             }
    //             PropertyKind::Assumption => {
    //                 let index = self.fsm.add_assume(named.value);
    //                 *self.fsm.get_assume_label_mut(index) = Some(sanitize_symbol(&named.name));
    //                 assumptions.push(named);
    //             }
    //             PropertyKind::Cover => {
    //                 let index = self.fsm.add_cover(named.value);
    //                 *self.fsm.get_cover_label_mut(index) = Some(sanitize_symbol(&named.name));
    //                 covers.push(named);
    //             }
    //         }
    //     }
    //     Ok((assertions, assumptions, covers))
    // }
}

// These generated zero assignments are represented by the formal latch reset values.
// Keep rejecting arbitrary RTL initialization rather than silently discarding it.
fn is_formal_static_initializer(design: &Design, statement: &Statement) -> bool {
    if statement.source.node_type != "INITIALSTATIC" {
        return false;
    }
    let StatementKind::Block { statements, .. } = &statement.kind else {
        return false;
    };
    statements.into_iter().all(|statement| {
        let StatementKind::Assignment {
            target: AssignmentTarget::Variable { variable, .. },
            value,
            ..
        } = &statement.kind
        else {
            return false;
        };
        let ExpressionKind::Constant(literal) = &value.kind else {
            return false;
        };
        formal_history_initial_value(design.variable(*variable)) == Some(false)
            && literal.value.bits() == 0
    })
}

fn formal_history_initial_value(variable: &Variable) -> Option<bool> {
    let formal_history = variable
        .original_name
        .as_deref()
        .and_then(|name| name.rsplit('.').next())
        .is_some_and(|name| name.starts_with("_Vpast_") || name.starts_with("__Vnfa_"));
    (variable.kind == VariableKind::ModuleTemporary && formal_history).then_some(false)
}

fn named_signal(
    ctx: &mut Context,
    design: &Design,
    variable: &Variable,
    values: &[ExprRef],
) -> NamedSignal {
    // let width = design.data_type(variable.dtype).indices.into_iter().count()
    // let symbol =
    // NamedSignal {
    //     name: variable.display_name().to_string(),
    //     bits: design
    //         .data_type(variable.dtype)
    //         .indices
    //         .into_iter()
    //         .zip(values.to_vec())
    //         .map(|(index, value)| NamedBit { index, value })
    //         .collect(),
    // }
    todo!()
}

fn resize(mut value: Vec<ExprRef>, width: usize, signed: bool, fill: ExprRef) -> Vec<ExprRef> {
    let fill = if signed {
        *value.last().unwrap_or(&fill)
    } else {
        fill
    };
    value.truncate(width);
    value.resize(width, fill);
    value
}

// fn usize_values(value: usize, width: usize) -> Vec<ExprRef> {
//     (0..width)
//         .map(|bit| ExprRef::Constant(bit < usize::BITS as usize && ((value >> bit) & 1) == 1))
//         .collect()
// }

fn apply_reset(model: &mut NamedFsm, reset: &SignalDomain) -> Result<(), ConvertError> {
    let reset_signal = (&model.inputs)
        .into_iter()
        .find(|signal| signal.name == reset.domain.name)
        .ok_or_else(|| {
            ConvertError::message(format!(
                "reset input {} was not converted",
                reset.domain.name
            ))
        })?;
    todo!()
    // if reset_signal.bits.len() != 1 {
    //     return Err(ConvertError::message(format!(
    //         "reset input {} must be one bit wide",
    //         reset.domain.name
    //     )));
    // }
    // let reset_variable = reset_signal.bits[0]
    //     .value
    //     .get_variable()
    //     .ok_or_else(|| ConvertError::message("reset input unexpectedly became constant"))?;
    // let asserted = reset.domain.edge == Edge::Positive;
    // let mut simulator = Simulator::from(model.fsm.clone());
    // simulator.set_input(reset_variable.index(), Some(asserted));
    // simulator.eval();
    // simulator.step();
    //
    // for latch in model.fsm.get_latches() {
    //     let reset_value = simulator
    //         .get_variable_signed(latch.output)
    //         .or(latch.reset_value);
    //     model
    //         .fsm
    //         .get_latch_mut(latch.output.index())
    //         .unwrap()
    //         .reset_value = reset_value;
    // }
    //
    // let (fsm, mapping) = rebuild_with_tied_input(&model.fsm, reset_variable.index(), !asserted);
    // model.fsm = fsm;
    // remap_model_values(model, &mapping);
    // model
    //     .inputs
    //     .retain(|signal| signal.name != reset.domain.name);
    // Ok(())
}
