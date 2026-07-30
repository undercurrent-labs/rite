//! The `@console` shortcut: it needs `&mut` context access, so it bypasses the capability host.

use super::*;
use crate::value::Value;

impl<'a> Evaluator<'a> {
    pub(super) async fn eval_console(
        &mut self,
        path: &[String],
        args: Vec<Value>,
    ) -> Result<Value, EvalError> {
        if !self.ctx.console_allowed && !self.ctx.allow_all {
            return Err(EvalError::Permission("console permission denied".into()));
        }
        let method = path.get(1).map(|s| s.as_str()).unwrap_or("print");
        let msg = args
            .first()
            .map(|v| v.to_display(&self.ctx.atoms))
            .unwrap_or_default();
        match method {
            "print" => {
                self.ctx.print(msg);
                Ok(Value::None)
            }
            "println" => {
                self.ctx.print(format!("{}\n", msg));
                Ok(Value::None)
            }
            "warn" | "error" => {
                self.ctx.print_err(format!("{}\n", msg));
                Ok(Value::None)
            }
            "inspect" => {
                self.ctx.print(format!("{:?}\n", args.first()));
                Ok(Value::None)
            }
            "read_line" => Ok(Value::string("")),
            _ => self.ctx.capabilities.call(path, args, true, self.ctx).await,
        }
    }
}
