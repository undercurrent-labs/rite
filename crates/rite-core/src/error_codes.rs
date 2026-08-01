//! Stable diagnostic error codes for Rite v1.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable error/warning code (e.g. E021).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ErrorCode(pub u16);

impl ErrorCode {
    pub const fn new(n: u16) -> Self {
        Self(n)
    }

    pub fn as_str(self) -> String {
        format!("E{:03}", self.0)
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E{:03}", self.0)
    }
}

// Lexical
pub const E001_INVALID_UTF8: ErrorCode = ErrorCode(1);
pub const E002_UNEXPECTED_CHAR: ErrorCode = ErrorCode(2);
pub const E003_UNTERMINATED_STRING: ErrorCode = ErrorCode(3);
pub const E004_UNTERMINATED_COMMENT: ErrorCode = ErrorCode(4);
pub const E005_INVALID_NUMBER: ErrorCode = ErrorCode(5);
pub const E006_INVALID_ESCAPE: ErrorCode = ErrorCode(6);

// Parse
pub const E010_UNEXPECTED_TOKEN: ErrorCode = ErrorCode(10);
pub const E011_EXPECTED_TOKEN: ErrorCode = ErrorCode(11);
pub const E012_UNCLOSED_DELIMITER: ErrorCode = ErrorCode(12);
pub const E013_INVALID_SYNTAX: ErrorCode = ErrorCode(13);
pub const E014_AMBIGUOUS_QUESTION: ErrorCode = ErrorCode(14);
/// A pipeline's result used as an operand: `xs → count > 2`.
///
/// `→` is looser than every binary operator so that its *input* reads as written
/// (`a + b → str` is `(a + b) → str`). Nothing below that level can consume a
/// trailing operator, so the parenthesised form is required and asked for by name
/// rather than left to fail somewhere else.
pub const E015_PIPELINE_RESULT_OPERAND: ErrorCode = ErrorCode(15);
/// `?` applied to a pipeline stage: `xs → f(a)?`.
///
/// Postfix `?` binds to the stage, so it unwraps the stage expression rather than
/// the value flowing through the pipeline — a reading with no use, and one the
/// interpreter and the compiler backend disagreed about. `?` goes on the result:
/// `(xs → f(a))?`.
pub const E016_TRY_ON_PIPELINE_STAGE: ErrorCode = ErrorCode(16);

// Resolve
pub const E020_UNDEFINED_NAME: ErrorCode = ErrorCode(20);
pub const E021_EFFECT_REQUIRED: ErrorCode = ErrorCode(21);
pub const E022_DUPLICATE_BINDING: ErrorCode = ErrorCode(22);
pub const E023_IMMUTABLE_ASSIGN: ErrorCode = ErrorCode(23);
pub const E024_IMPORT_CYCLE: ErrorCode = ErrorCode(24);
pub const E025_PRIVATE_IMPORT: ErrorCode = ErrorCode(25);
pub const E026_MODULE_NOT_FOUND: ErrorCode = ErrorCode(26);
pub const E027_INVALID_ARITY: ErrorCode = ErrorCode(27);
pub const E028_TYPE_CONTRACT: ErrorCode = ErrorCode(28);
pub const E029_NON_EXHAUSTIVE_MATCH: ErrorCode = ErrorCode(29);

// Runtime
pub const E030_RUNTIME: ErrorCode = ErrorCode(30);
pub const E031_TYPE_ERROR: ErrorCode = ErrorCode(31);
pub const E032_ARITHMETIC_OVERFLOW: ErrorCode = ErrorCode(32);
pub const E033_INDEX_OUT_OF_BOUNDS: ErrorCode = ErrorCode(33);
pub const E034_DIVISION_BY_ZERO: ErrorCode = ErrorCode(34);
pub const E035_PANIC: ErrorCode = ErrorCode(35);
pub const E036_BUDGET_EXCEEDED: ErrorCode = ErrorCode(36);
pub const E037_CANCELLED: ErrorCode = ErrorCode(37);
pub const E038_MATCH_FAILURE: ErrorCode = ErrorCode(38);
pub const E039_STACK_OVERFLOW: ErrorCode = ErrorCode(39);

// Capabilities / permissions
pub const E040_PERMISSION_DENIED: ErrorCode = ErrorCode(40);
pub const E041_CAPABILITY_ERROR: ErrorCode = ErrorCode(41);
pub const E042_UNKNOWN_CAPABILITY: ErrorCode = ErrorCode(42);
pub const E043_PATH_TRAVERSAL: ErrorCode = ErrorCode(43);

// Compile
pub const E050_COMPILE_UNSUPPORTED: ErrorCode = ErrorCode(50);
pub const E051_COMPILE_FAILURE: ErrorCode = ErrorCode(51);
pub const E052_CODEGEN: ErrorCode = ErrorCode(52);

// HTTP
pub const E060_HTTP: ErrorCode = ErrorCode(60);
pub const E061_ROUTE_CONFLICT: ErrorCode = ErrorCode(61);

// Test
pub const E070_TEST_FAILURE: ErrorCode = ErrorCode(70);
pub const E071_ASSERTION: ErrorCode = ErrorCode(71);

// IO / config
pub const E080_IO: ErrorCode = ErrorCode(80);
pub const E081_USAGE: ErrorCode = ErrorCode(81);
