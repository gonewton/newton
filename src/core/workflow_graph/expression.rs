#![allow(clippy::result_large_err)] // Expression engine returns AppError to preserve compile/eval diagnostics without boxing.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use rhai::{Array, Dynamic, Engine, Map, Scope, AST};
use serde_json::{Map as JsonMap, Number, Value};

/// Context variables exposed to expressions.
#[derive(Clone)]
pub struct EvaluationContext {
    pub context: Value,
    pub tasks: Value,
    pub triggers: Value,
}

impl EvaluationContext {
    pub fn new(context: Value, tasks: Value, triggers: Value) -> Self {
        Self {
            context,
            tasks,
            triggers,
        }
    }
}

/// Expression evaluation engine using a locked-down Rhai configuration.
pub struct ExpressionEngine {
    engine: Engine,
}

impl Default for ExpressionEngine {
    fn default() -> Self {
        let mut engine = Engine::new_raw();
        engine.set_max_operations(50_000);
        engine.set_max_call_levels(64);
        engine.set_max_expr_depths(64, 64);
        engine.on_print(|_| {});
        engine.on_debug(|_, _, _| {});
        ExpressionEngine { engine }
    }
}

impl ExpressionEngine {
    /// Compile the given expression string into an AST.
    pub fn compile(&self, expr: &str) -> Result<AST, AppError> {
        self.engine.compile(expr).map_err(|err| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("expression compile error: {}", err),
            )
            .with_code("WFG-EXPR-001")
        })
    }

    /// Evaluate the given expression string against the provided context.
    pub fn evaluate(&self, expr: &str, ctx: &EvaluationContext) -> Result<Value, AppError> {
        let mut scope = Scope::new();
        populate_scope(&mut scope, ctx);
        let result = self
            .engine
            .eval_with_scope::<Dynamic>(&mut scope, expr)
            .map_err(|err| {
                AppError::new(
                    ErrorCategory::ValidationError,
                    format!("expression execution error: {}", err),
                )
                .with_code("WFG-EXPR-001")
            })?;
        Ok(from_dynamic(result))
    }

    /// Interpolate `{{ expr }}` segments in a string using the evaluation context.
    pub fn interpolate_string(
        &self,
        value: &str,
        ctx: &EvaluationContext,
    ) -> Result<String, AppError> {
        if !value.contains("{{") {
            return Ok(value.to_string());
        }
        let mut result = String::new();
        let mut remaining = value;
        while let Some(start) = remaining.find("{{") {
            result.push_str(&remaining[..start]);
            let after_start = &remaining[start + 2..];
            let end = after_start.find("}}").ok_or_else(|| {
                AppError::new(
                    ErrorCategory::ValidationError,
                    "missing closing '}}' in template string",
                )
                .with_code("WFG-TPL-001")
            })?;
            let expr = after_start[..end].trim();
            if expr.is_empty() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "empty template interpolation expression",
                )
                .with_code("WFG-TPL-001"));
            }
            self.compile(expr).map_err(|err| {
                AppError::new(
                    ErrorCategory::ValidationError,
                    format!("template interpolation compile error: {}", err.message),
                )
                .with_code("WFG-TPL-001")
            })?;
            let mut scope = Scope::new();
            populate_scope(&mut scope, ctx);
            let dynamic = self
                .engine
                .eval_with_scope::<Dynamic>(&mut scope, expr)
                .map_err(|err| {
                    AppError::new(
                        ErrorCategory::ValidationError,
                        format!("template interpolation execution error: {}", err),
                    )
                    .with_code("WFG-TPL-001")
                })?;
            let json_value = from_dynamic(dynamic);
            match json_value {
                Value::String(text) => result.push_str(&text),
                other => {
                    let encoded = serde_json::to_string(&other).map_err(|err| {
                        AppError::new(
                            ErrorCategory::SerializationError,
                            format!("template interpolation stringify failed: {}", err),
                        )
                        .with_code("WFG-TPL-001")
                    })?;
                    result.push_str(&encoded);
                }
            }
            remaining = &after_start[end + 2..];
        }
        result.push_str(remaining);
        Ok(result)
    }
}

fn populate_scope(scope: &mut Scope<'_>, ctx: &EvaluationContext) {
    scope.push_dynamic("context", to_dynamic(&ctx.context));
    scope.push_dynamic("tasks", to_dynamic(&ctx.tasks));
    scope.push_dynamic("triggers", to_dynamic(&ctx.triggers));
    if let Some(map) = ctx.context.as_object() {
        for (key, value) in map {
            scope.push_dynamic(key.clone(), to_dynamic(value));
        }
    }
}

fn to_dynamic(value: &Value) -> Dynamic {
    match value {
        Value::Null => Dynamic::UNIT,
        Value::Bool(b) => Dynamic::from(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Dynamic::from(i)
            } else if let Some(u) = n.as_u64() {
                Dynamic::from(u)
            } else if let Some(f) = n.as_f64() {
                Dynamic::from(f)
            } else {
                Dynamic::from(0_i64)
            }
        }
        Value::String(s) => Dynamic::from(s.clone()),
        Value::Array(items) => {
            let mut arr = Array::new();
            for item in items {
                arr.push(to_dynamic(item));
            }
            Dynamic::from_array(arr)
        }
        Value::Object(map) => {
            let mut rhai_map = Map::new();
            for (key, value) in map {
                rhai_map.insert(key.into(), to_dynamic(value));
            }
            Dynamic::from_map(rhai_map)
        }
    }
}

fn from_dynamic(value: Dynamic) -> Value {
    if value.is_unit() {
        return Value::Null;
    }
    if let Some(b) = value.clone().try_cast::<bool>() {
        return Value::Bool(b);
    }
    if let Some(i) = value.clone().try_cast::<i64>() {
        return Value::Number(Number::from(i));
    }
    if let Some(u) = value.clone().try_cast::<u64>() {
        return Value::Number(Number::from(u));
    }
    if let Some(f) = value.clone().try_cast::<f64>() {
        if let Some(num) = Number::from_f64(f) {
            return Value::Number(num);
        }
    }
    if let Some(s) = value.clone().try_cast::<String>() {
        return Value::String(s);
    }
    if let Some(arr) = value.clone().try_cast::<Array>() {
        return Value::Array(arr.into_iter().map(from_dynamic).collect());
    }
    if let Some(map) = value.clone().try_cast::<Map>() {
        let mut json_map = JsonMap::new();
        for (key, value) in map {
            json_map.insert(key.into(), from_dynamic(value));
        }
        return Value::Object(json_map);
    }
    Value::Null
}
