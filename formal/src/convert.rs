pub use crate::error::ConvertError;
use clap::builder::Str;
use parser_verilator::ast::DataTypeKind::PackedArray;
use parser_verilator::ast::{PropertyKind, SourceInfo};
use parser_verilator::{
    ast::{
        AssignmentKind, AssignmentTarget, BinaryOperator, Design, Direction, Domain, Expression,
        ExpressionKind, SignalDomain, Statement, StatementKind, UnaryOperator, Variable,
        VariableId, VariableKind, collect::CollectAccesses, sequential,
    },
    document::AstDocument,
};
use patronus::expr::{Context, ExprRef, SerializableIrNode, TypeCheck, WidthInt};
use patronus::system::{Output, State, TransitionSystem};
use std::cmp::Ordering;
use std::{
    collections::{BTreeMap, BTreeSet},
    mem,
};

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
    pub properties: Properties,
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

type Environment = BTreeMap<VariableId, ExprRef>;

struct Converter<'a> {
    design: &'a Design,
    clock: SignalDomain,
    reset: Option<SignalDomain>,
    sys: TransitionSystem,
    pending: Environment,
}

impl NamedFsm {
    pub fn from_design(
        ctx: &mut Context,
        design: &Design,
        clock: Domain,
        reset: Option<Domain>,
    ) -> Result<Self, ConvertError> {
        let (clock, reset) = select_domains(design, Some(&clock), reset.as_ref())?;
        let (sys, properties) = Converter {
            design,
            clock: clock.clone(),
            reset: reset.clone(),
            sys: TransitionSystem::new("todo".into()),
            pending: Environment::new(),
        }
        .convert(ctx)?;
        Ok(Self {
            sys,
            clock,
            reset,
            properties,
        })
    }

    pub fn from_document(
        ctx: &mut Context,
        document: &AstDocument,
        clock: Domain,
        reset: Option<Domain>,
    ) -> Result<Self, ConvertError> {
        let design = Design::try_from(document)?;
        Self::from_design(ctx, &design, clock, reset)
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

#[derive(Debug, Default, Clone)]
pub struct Properties {
    pub asserts: Vec<String>,
    pub assumes: Vec<String>,
    pub covers: Vec<String>,
    pub cover_exprs: Vec<ExprRef>,
}

type Covers = Vec<(String, ExprRef)>;

impl Converter<'_> {
    fn analyse_rw(&self) -> (BTreeSet<VariableId>, BTreeSet<VariableId>) {
        let all_statements = (&self.design.combinational)
            .into_iter()
            .chain(&self.design.sequential)
            .cloned()
            .collect::<Vec<_>>();
        let mut reads = BTreeSet::new();
        let mut writes = BTreeSet::new();
        all_statements
            .as_slice()
            .collect_accesses(&mut reads, &mut writes);

        // TODO: why is this loop necessary
        loop {
            let before = reads.len();
            for (index, variable) in (&self.design.variables).into_iter().enumerate() {
                let id = VariableId(index);
                if reads.contains(&id)
                    && let Some(sampled) = &variable.sampled_value
                {
                    sampled.collect_reads(&mut reads);
                }
            }
            if reads.len() == before {
                break;
            }
        }

        (reads, writes)
    }

    fn convert(
        mut self,
        ctx: &mut Context,
    ) -> Result<(TransitionSystem, Properties), ConvertError> {
        // make sure that we can deal with all initial statements in the source
        if let Some(initial) = (&self.design.initial)
            .into_iter()
            .find(|statement| !is_formal_static_initializer(self.design, statement))
        {
            return Err(ConvertError::source(
                &initial.source,
                "initial blocks are not supported by AIGER conversion",
            ));
        }

        // calculate read/write set
        let (reads, writes) = self.analyse_rw();

        // determine registers
        let sequential = sequential::analyze(self.design).map_err(ConvertError::message)?;
        let mut register_ids = sequential.registers.clone();

        // TODO: what does this mean?
        // Preserve the existing unpacked-array storage model for procedural
        // combinational element writes, which read the untouched elements.
        register_ids.extend(
            (&self.design.variables)
                .into_iter()
                .enumerate()
                .filter(|(index, variable)| {
                    self.design.data_type(variable.dtype).unpacked.is_some()
                        && reads.contains(&VariableId(*index))
                        && writes.contains(&VariableId(*index))
                })
                .map(|(index, _)| VariableId(index)),
        );

        // find inputs
        let input_ids: Vec<_> = self
            .design
            .variables
            .iter()
            .enumerate()
            .filter_map(|(index, variable)| {
                let id = VariableId(index);
                let primary_input = variable.direction == Direction::Input;
                // TODO: what causes undriven reads?
                let undriven_read = reads.contains(&id)
                    && !writes.contains(&id)
                    && !register_ids.contains(&id)
                    && variable.sampled_value.is_none()
                    && !variable.internal;
                ((primary_input || undriven_read) && id != self.clock.variable).then_some(id)
            })
            .collect();

        // the environment maps variables from the design to the transition system
        let mut environment = Environment::new();

        // add inputs to transition system
        self.sys.inputs = input_ids
            .into_iter()
            .map(|id| {
                let variable = self.design.variable(id);
                let width = self.design.data_type(variable.dtype).width as WidthInt;
                let sym = ctx.bv_symbol(&variable.name, width);
                environment.insert(id, sym);
                sym
            })
            .collect();

        // add states to transition system (for now without next state or initial value)
        self.sys.states = register_ids
            .iter()
            .map(|&id| {
                let variable = self.design.variable(id);
                let width = self.design.data_type(variable.dtype).width as WidthInt;
                let symbol = ctx.bv_symbol(&variable.name, width);
                environment.insert(id, symbol);
                State {
                    symbol,
                    init: None,
                    next: None,
                }
            })
            .collect();

        // TODO: it is unclear how this combinational logic analysis works
        self.execute_combinational(ctx, &self.design.combinational, &mut environment)?;

        // Freeze sampled expressions before any clocked blocking assignments run.
        let before_edge = environment.clone();
        for (index, variable) in (&self.design.variables).into_iter().enumerate() {
            if let Some(sampled) = &variable.sampled_value {
                let value = self.on_bv_expr(ctx, sampled, &before_edge)?;
                environment.insert(VariableId(index), value);
            }
        }
        for id in &sequential.nonblocking {
            self.pending.insert(*id, environment[id].clone());
        }

        for stmt in &self.design.sequential {
            self.on_stmt(ctx, stmt, &mut environment)?;
        }

        for (register, state) in register_ids.iter().zip(self.sys.states.iter_mut()) {
            let next = self
                .pending
                .get(register)
                .or_else(|| environment.get(register))
                .cloned()
                .ok_or_else(|| ConvertError::message("register has no next value"))?;

            state.next = Some(next);
            if is_formal_history_register(self.design.variable(*register)) {
                state.init = Some(ctx.zero(state.symbol.get_bv_type(ctx).unwrap()));
            }
        }

        // reset register next state to current state for output calculation
        for (register, state) in register_ids.iter().zip(self.sys.states.iter()) {
            environment.insert(*register, state.symbol);
        }

        for (index, variable) in (&self.design.variables).into_iter().enumerate() {
            if variable.direction != Direction::Output {
                continue;
            }
            let id = VariableId(index);
            let expr = environment.get(&id).cloned().ok_or_else(|| {
                ConvertError::source(&variable.source, "output has no symbolic value")
            })?;
            let name = ctx.string(variable.display_name().into());
            self.sys.outputs.push(Output { name, expr });
        }

        let props = self.add_properties(ctx, &environment)?;

        Ok((self.sys, props))
    }

    fn execute_combinational(
        &mut self,
        ctx: &mut Context,
        statements: &[Statement],
        environment: &mut Environment,
    ) -> Result<(), ConvertError> {
        let mut pending = statements.into_iter().collect::<Vec<_>>();
        while !pending.is_empty() {
            let Some(index) = (&pending).into_iter().position(|statement| {
                let mut reads = BTreeSet::new();
                statement.collect_reads(&mut reads);
                reads.into_iter().all(|id| environment.contains_key(&id))
            }) else {
                return Err(ConvertError::message(
                    "combinational logic has a cycle or unresolved input",
                ));
            };
            let statement = pending.remove(index);
            self.on_stmt(ctx, statement, environment).map_err(|error| {
                ConvertError::source(
                    &statement.source,
                    format!("combinational evaluation failed: {error}"),
                )
            })?;
        }
        Ok(())
    }

    fn on_stmt(
        &mut self,
        ctx: &mut Context,
        statement: &Statement,
        environment: &mut Environment,
    ) -> Result<(), ConvertError> {
        match &statement.kind {
            StatementKind::Block { statements, .. } => {
                for stmt in statements {
                    self.on_stmt(ctx, stmt, environment)?;
                }
                Ok(())
            }
            StatementKind::Assignment {
                kind,
                target,
                value,
            } => {
                let value = self.on_bv_expr(ctx, value, environment)?;
                if *kind == AssignmentKind::Nonblocking {
                    let mut pending = mem::take(&mut self.pending);
                    let result = self.assign(ctx, target, value, &mut pending, environment);
                    self.pending = pending;
                    result
                } else {
                    debug_assert_eq!(*kind, AssignmentKind::Blocking);
                    let evaluation = environment.clone();
                    self.assign(ctx, target, value, environment, &evaluation)
                }
            }
            StatementKind::If {
                condition,
                then_statements,
                else_statements,
            } => {
                let condition_value = self.on_bv_expr(ctx, condition, environment)?;
                let condition = self.truthy(ctx, condition_value);
                let before = environment.clone();
                let pending_before = self.pending.clone();

                // true branch
                let mut then_environment = before.clone();
                for stmt in then_statements {
                    self.on_stmt(ctx, stmt, &mut then_environment)?;
                }
                let then_pending = mem::replace(&mut self.pending, pending_before);

                // false branch
                let mut else_environment = before.clone();
                for stmt in else_statements {
                    self.on_stmt(ctx, stmt, &mut else_environment)?;
                }
                let else_pending = mem::take(&mut self.pending);

                // reconcile environments after branch
                self.pending = self.merge_environments(
                    ctx,
                    &statement.source,
                    condition,
                    &then_pending,
                    &else_pending,
                    &Environment::new(),
                )?;
                *environment = self.merge_environments(
                    ctx,
                    &statement.source,
                    condition,
                    &then_environment,
                    &else_environment,
                    &before,
                )?;
                Ok(())
            }
        }
    }

    /// Merges the divergent program state after and if/else statement
    fn merge_environments(
        &mut self,
        ctx: &mut Context,
        source: &SourceInfo,
        condition: ExprRef,
        then_environment: &Environment,
        else_environment: &Environment,
        before: &Environment,
    ) -> Result<Environment, ConvertError> {
        let mut environment = Environment::new();
        let keys = then_environment
            .keys()
            .chain(else_environment.keys())
            .copied()
            .collect::<BTreeSet<_>>();
        for key in keys {
            let width = self
                .design
                .variables
                .get(key.0)
                .map_or(0, |variable| self.design.data_type(variable.dtype).width)
                as WidthInt;

            // TODO: why is there a zero default?
            let default = ctx.zero(width);
            let then_value = then_environment
                .get(&key)
                .or_else(|| before.get(&key))
                .cloned()
                .unwrap_or(default);
            let else_value = else_environment
                .get(&key)
                .or_else(|| before.get(&key))
                .cloned()
                .unwrap_or(default);
            if then_value == else_value {
                environment.insert(key, then_value.clone());
            } else {
                if ctx[then_value].get_bv_type(ctx) != ctx[else_value].get_bv_type(ctx) {
                    return Err(ConvertError::source(source, "IF branch width mismatch"));
                }
                environment.insert(key, ctx.ite(condition, then_value, else_value));
            }
        }
        Ok(environment)
    }

    fn assign(
        &mut self,
        ctx: &mut Context,
        target: &AssignmentTarget,
        value: ExprRef,
        environment: &mut Environment,
        evaluation: &Environment,
    ) -> Result<(), ConvertError> {
        match target {
            &AssignmentTarget::Variable { variable, .. } => {
                let width = self
                    .design
                    .data_type(self.design.variable(variable).dtype)
                    .width as WidthInt;
                environment.insert(variable, ext_or_truncate(ctx, value, width, false));
                Ok(())
            }
            AssignmentTarget::Select {
                target,
                offset,
                width,
                ..
            } => {
                todo!("learn about selects and implement them")
                // let target_value = self.read_target(target, environment, evaluation)?;
                // if value.len() != *width || *width > target_value.len() {
                //     return Err(ConvertError::source(
                //         &offset.source,
                //         "SEL assignment width mismatch",
                //     ));
                // }
                // if let ExpressionKind::Constant(literal) = &offset.kind {
                //     let offset = usize::try_from(&literal.value).unwrap();
                //     let mut result = target_value;
                //     result[offset..offset + width].copy_from_slice(&value);
                //     return self.assign(target, result, environment, evaluation);
                // }
                // let offset_value = self.expression(offset, evaluation)?;
                // let mut result = target_value.clone();
                // for candidate_offset in 0..=target_value.len() - width {
                //     let candidate_value = usize_values(candidate_offset, offset_value.len());
                //     let selected = self.equals_constant_without_or(&offset_value, &candidate_value);
                //     let mut candidate = target_value.clone();
                //     candidate[candidate_offset..candidate_offset + width].copy_from_slice(&value);
                //     result = self.mux_without_or(&result, &candidate, selected);
                // }
                // self.assign(target, result, environment, evaluation)
            }
            AssignmentTarget::ArrayElement {
                array,
                index,
                dtype,
            } => {
                todo!("array assignments might need to be handled differently")
            }
        }
    }

    fn read_target(
        &mut self,
        target: &AssignmentTarget,
        environment: &Environment,
        evaluation: &Environment,
    ) -> Result<ExprRef, ConvertError> {
        match target {
            &AssignmentTarget::Variable { variable, .. } => {
                environment.get(&variable).cloned().ok_or_else(|| {
                    ConvertError::message(format!(
                        "assignment target {} is unresolved",
                        self.design.variable(variable).display_name()
                    ))
                })
            }
            AssignmentTarget::Select { offset, width, .. } => {
                let AssignmentTarget::Select { target, .. } = target else {
                    unreachable!()
                };
                todo!("select")
                // let source = self.read_target(target, environment, evaluation)?;
                // self.select_value(source, offset, *width, evaluation)
            }
            AssignmentTarget::ArrayElement {
                array,
                index,
                dtype,
            } => {
                todo!("array read")
                // let source = self.read_target(array, environment, evaluation)?;
                // self.array_select_value(source, index, self.design.data_type(*dtype), evaluation)
            }
        }
    }

    fn on_bv_expr(
        &mut self,
        ctx: &mut Context,
        expression: &Expression,
        environment: &Environment,
    ) -> Result<ExprRef, ConvertError> {
        let width = self.design.data_type(expression.dtype).width as WidthInt;
        match &expression.kind {
            ExpressionKind::Constant(literal) => {
                let value = baa::BitVecValue::from_big_uint(&literal.value, width);
                Ok(ctx.bv_lit(&value.into()))
            }
            ExpressionKind::Variable { variable, .. } => {
                if let Some(&value) = environment.get(variable) {
                    return Ok(value);
                }
                // TODO: what does sampled mean in this context?
                if let Some(sampled) = &self.design.variable(*variable).sampled_value {
                    return self.on_bv_expr(ctx, sampled, environment);
                }
                Err(ConvertError::source(
                    &expression.source,
                    format!(
                        "unresolved symbolic variable {}",
                        self.design.variable(*variable).display_name()
                    ),
                ))
            }
            ExpressionKind::Unary { operator, operand } => {
                self.unary_expression(ctx, expression, *operator, operand, environment)
            }
            ExpressionKind::Binary { operator, lhs, rhs } => {
                self.binary_expression(ctx, expression, *operator, lhs, rhs, environment)
            }
            ExpressionKind::Conditional {
                condition,
                then_value,
                else_value,
            } => {
                let condition_value = self.on_bv_expr(ctx, condition, environment)?;
                let select = self.truthy(ctx, condition_value);
                let then_value = self.on_bv_expr(ctx, then_value, environment)?;
                let then_value = ext_or_truncate(ctx, then_value, width, false);
                let else_value = self.on_bv_expr(ctx, else_value, environment)?;
                let else_value = ext_or_truncate(ctx, else_value, width, false);
                Ok(ctx.ite(select, then_value, else_value))
            }
            ExpressionKind::Replicate { source, count, .. } => {
                let source = self.on_bv_expr(ctx, source, environment)?;
                let source_width = ctx[source].get_bv_type(ctx).unwrap();
                assert_eq!(source_width * *count as u32, width);
                let mut out = source;
                for _ in 1..*count {
                    out = ctx.concat(out, source);
                }
                Ok(out)
            }
            ExpressionKind::Select {
                value,
                offset,
                width,
            } => {
                let value = self.on_bv_expr(ctx, value, environment)?;
                todo!("select")
                //self.select_value(ctx, value, offset, *width, environment)
            }
            ExpressionKind::ArraySelect { array, index } => {
                let value = self.on_bv_expr(ctx, array, environment)?;
                let index = self.on_bv_expr(ctx, index, environment)?;
                todo!("array select")
            }
        }
    }

    fn unary_expression(
        &mut self,
        ctx: &mut Context,
        expression: &Expression,
        operator: UnaryOperator,
        operand: &Expression,
        environment: &Environment,
    ) -> Result<ExprRef, ConvertError> {
        let expr = self.on_bv_expr(ctx, operand, environment)?;
        let width = self.design.data_type(expression.dtype).width as WidthInt;
        Ok(match operator {
            UnaryOperator::BitwiseNot | UnaryOperator::Negate => {
                let in_width = ctx[expr].get_bv_type(ctx).unwrap();
                assert_eq!(in_width, width, "TODO: zero or sign extend");
                match operator {
                    UnaryOperator::BitwiseNot => ctx.not(expr),
                    UnaryOperator::Negate => ctx.negate(expr),
                    _ => unreachable!(),
                }
            }
            UnaryOperator::LogicalNot => {
                let bit = self.truthy(ctx, expr);
                ctx.not(bit)
            }
            UnaryOperator::ReduceAnd => todo!("and reductions"),
            UnaryOperator::ReduceOr => todo!("or reductions"),
            UnaryOperator::ReduceXor => todo!("xor reductions"),
            UnaryOperator::ZeroExtend => ext_or_truncate(ctx, expr, width, false),
            UnaryOperator::SignExtend => ext_or_truncate(ctx, expr, width, true),
        })
    }

    fn binary_expression(
        &mut self,
        ctx: &mut Context,
        expression: &Expression,
        operator: BinaryOperator,
        lhs: &Expression,
        rhs: &Expression,
        environment: &Environment,
    ) -> Result<ExprRef, ConvertError> {
        let width = self.design.data_type(expression.dtype).width as WidthInt;
        let lhs_value = self.on_bv_expr(ctx, lhs, environment)?;
        let rhs_value = self.on_bv_expr(ctx, rhs, environment)?;
        Ok(match operator {
            BinaryOperator::BitwiseAnd | BinaryOperator::BitwiseOr | BinaryOperator::BitwiseXor => {
                let lhs = ext_or_truncate(ctx, lhs_value, width, false);
                let rhs = ext_or_truncate(ctx, rhs_value, width, false);
                match operator {
                    BinaryOperator::BitwiseAnd => ctx.and(lhs, rhs),
                    BinaryOperator::BitwiseOr => ctx.or(lhs, rhs),
                    BinaryOperator::BitwiseXor => ctx.xor(lhs, rhs),
                    _ => unreachable!(),
                }
            }
            BinaryOperator::Add | BinaryOperator::Subtract => {
                let signed = self.design.data_type(expression.dtype).signed;
                let lhs = ext_or_truncate(ctx, lhs_value, width, signed);
                let rhs = ext_or_truncate(ctx, rhs_value, width, signed);
                if operator == BinaryOperator::Add {
                    ctx.add(lhs, rhs)
                } else {
                    ctx.sub(lhs, rhs)
                }
            }
            BinaryOperator::MultiplyUnsigned | BinaryOperator::MultiplySigned => {
                let signed = operator == BinaryOperator::MultiplySigned;
                let lhs = ext_or_truncate(ctx, lhs_value, width, signed);
                let rhs = ext_or_truncate(ctx, rhs_value, width, signed);
                ctx.mul(lhs, rhs)
            }
            BinaryOperator::DivideUnsigned => {
                let lhs = ext_or_truncate(ctx, lhs_value, width, false);
                let rhs = ext_or_truncate(ctx, rhs_value, width, false);
                ctx.div(lhs, rhs)
            }
            BinaryOperator::DivideSigned => {
                let lhs = ext_or_truncate(ctx, lhs_value, width, true);
                let rhs = ext_or_truncate(ctx, rhs_value, width, true);
                ctx.signed_div(lhs, rhs)
            }
            BinaryOperator::Equal
            | BinaryOperator::NotEqual
            | BinaryOperator::LessThanUnsigned
            | BinaryOperator::LessThanOrEqualUnsigned
            | BinaryOperator::GreaterThanUnsigned
            | BinaryOperator::GreaterThanOrEqualUnsigned
            | BinaryOperator::LessThanSigned
            | BinaryOperator::LessThanOrEqualSigned
            | BinaryOperator::GreaterThanSigned
            | BinaryOperator::GreaterThanOrEqualSigned => {
                let lhs_value_width = ctx[lhs_value].get_bv_type(ctx).unwrap();
                let rhs_value_width = ctx[rhs_value].get_bv_type(ctx).unwrap();
                let width = lhs_value_width.max(rhs_value_width);
                let signed = operator == BinaryOperator::LessThanSigned
                    || operator == BinaryOperator::LessThanOrEqualSigned
                    || operator == BinaryOperator::GreaterThanSigned
                    || operator == BinaryOperator::GreaterThanOrEqualSigned;
                let lhs = ext_or_truncate(ctx, lhs_value, width, signed);
                let rhs = ext_or_truncate(ctx, rhs_value, width, signed);
                match operator {
                    BinaryOperator::Equal => ctx.equal(lhs, rhs),
                    BinaryOperator::NotEqual => ctx.build(|b| b.not(b.equal(lhs, rhs))),
                    // a < b <=> b > a
                    BinaryOperator::LessThanUnsigned => ctx.greater(rhs, lhs),
                    BinaryOperator::LessThanSigned => ctx.greater_signed(rhs, lhs),
                    // a <= b <=> b >= a
                    BinaryOperator::LessThanOrEqualUnsigned => ctx.greater_or_equal(rhs, lhs),
                    BinaryOperator::LessThanOrEqualSigned => ctx.greater_or_equal_signed(rhs, lhs),
                    BinaryOperator::GreaterThanUnsigned => ctx.greater(lhs, rhs),
                    BinaryOperator::GreaterThanSigned => ctx.greater_signed(lhs, rhs),
                    BinaryOperator::GreaterThanOrEqualUnsigned => ctx.greater_or_equal(lhs, rhs),
                    BinaryOperator::GreaterThanOrEqualSigned => {
                        ctx.greater_or_equal_signed(lhs, rhs)
                    }
                    _ => unreachable!(),
                }
            }
            BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr => {
                let lhs = self.truthy(ctx, lhs_value);
                let rhs = self.truthy(ctx, rhs_value);
                if operator == BinaryOperator::LogicalAnd {
                    ctx.and(lhs, rhs)
                } else {
                    ctx.or(lhs, rhs)
                }
            }
            BinaryOperator::ShiftLeft
            | BinaryOperator::ShiftRight
            | BinaryOperator::ShiftRightArithmetic => {
                let lhs = ext_or_truncate(ctx, lhs_value, width, false);
                let rhs = ext_or_truncate(ctx, rhs_value, width, false);
                match operator {
                    BinaryOperator::ShiftLeft => ctx.shift_left(lhs, rhs),
                    BinaryOperator::ShiftRight => ctx.shift_right(lhs, rhs),
                    BinaryOperator::ShiftRightArithmetic => ctx.arithmetic_shift_right(lhs, rhs),
                    _ => unreachable!(),
                }
            }
            BinaryOperator::Concat => ctx.concat(lhs_value, rhs_value),
        })
    }

    fn select_value(
        &mut self,
        ctx: &mut Context,
        value: ExprRef,
        offset: ExprRef,
        width: usize,
        environment: &Environment,
    ) -> Result<Vec<ExprRef>, ConvertError> {
        todo!("figure out how selects work and implement them")

        //
        // if let ExpressionKind::Constant(literal) = &offset.kind {
        //     let offset = usize::try_from(&literal.value).unwrap();
        //     return Ok(value[offset..offset + width].to_vec());
        // }
        // let offset_value = self.expression(offset, environment)?;
        // let choice_count = value.len().checked_next_power_of_two().ok_or_else(|| {
        //     ConvertError::source(&offset.source, "dynamic selection input is too wide")
        // })?;
        // let useful_offset_bits = choice_count.trailing_zeros() as usize;
        // let low_offset = resize(
        //     offset_value.clone(),
        //     useful_offset_bits,
        //     false,
        //     ExprRef::Constant(false),
        // );
        // let mut result = (0..width)
        //     .map(|bit| {
        //         let choices = (0..choice_count)
        //             .map(|candidate_offset| {
        //                 value
        //                     .get(candidate_offset + bit)
        //                     .copied()
        //                     .unwrap_or(ExprRef::Constant(false))
        //             })
        //             .collect::<Vec<_>>();
        //         self.select_without_or(&choices, &low_offset)
        //     })
        //     .collect::<Vec<_>>();
        // if offset_value.len() > useful_offset_bits {
        //     let oversized = self.truthy(&offset_value[useful_offset_bits..]);
        //     let zero = vec![ExprRef::Constant(false); width];
        //     result = FsmOps::create_mux(&mut self.fsm, &result, &zero, oversized);
        // }
        // Ok(result)
    }
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
    fn truthy(&mut self, ctx: &mut Context, value: ExprRef) -> ExprRef {
        if ctx[value].get_bv_type(ctx).unwrap() == 1 {
            value
        } else {
            todo!("implement or reduction")
        }
    }
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
    fn add_properties(
        &mut self,
        ctx: &mut Context,
        environment: &Environment,
    ) -> Result<Properties, ConvertError> {
        let mut p = Properties::default();
        for (index, variable) in (&self.design.variables).into_iter().enumerate() {
            let Some(property) = &variable.property else {
                continue;
            };
            let value = environment
                .get(&VariableId(index))
                .cloned()
                .ok_or_else(|| {
                    ConvertError::source(&variable.source, "formal wire has no symbolic value")
                })?;
            if value.get_bv_type(ctx) != Some(1) {
                return Err(ConvertError::source(
                    &variable.source,
                    "formal wire is not one bit",
                ));
            }
            let name = property.name.clone();

            // TODO: add a way to name properties to the TransitionSystem API
            match property.kind {
                PropertyKind::Assertion => {
                    self.sys.bad_states.push(ctx.not(value));
                    p.asserts.push(name);
                }
                PropertyKind::Assumption => {
                    self.sys.constraints.push(value);
                    p.assumes.push(name);
                }
                PropertyKind::Cover => {
                    p.cover_exprs.push(value);
                    p.covers.push(name);
                }
            }
        }
        Ok(p)
    }
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
        is_formal_history_register(design.variable(*variable)) && literal.value.bits() == 0
    })
}

/// formal history registers need to be initialized to zero
fn is_formal_history_register(variable: &Variable) -> bool {
    let formal_history = variable
        .original_name
        .as_deref()
        .and_then(|name| name.rsplit('.').next())
        .is_some_and(|name| name.starts_with("_Vpast_") || name.starts_with("__Vnfa_"));
    variable.kind == VariableKind::ModuleTemporary && formal_history
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

fn ext_or_truncate(ctx: &mut Context, e: ExprRef, out_width: WidthInt, signed: bool) -> ExprRef {
    let in_width = ctx[e].get_bv_type(ctx).unwrap();
    match in_width.cmp(&out_width) {
        Ordering::Less if signed => ctx.sign_extend(e, out_width - in_width),
        Ordering::Less => ctx.zero_extend(e, out_width - in_width),
        Ordering::Equal => e,
        Ordering::Greater => ctx.slice(e, out_width - 1, 0),
    }
}

fn apply_reset(model: &mut NamedFsm, reset: &SignalDomain) -> Result<(), ConvertError> {
    // let reset_signal = (&model.inputs)
    //     .into_iter()
    //     .find(|signal| signal.name == reset.domain.name)
    //     .ok_or_else(|| {
    //         ConvertError::message(format!(
    //             "reset input {} was not converted",
    //             reset.domain.name
    //         ))
    //     })?;
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
