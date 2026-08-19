; Adapted from tree-sitter-swift's MIT-licensed highlights query.

[
  (comment)
  (multiline_comment)
] @comment

[
  (line_str_text)
  (multi_line_str_text)
  (raw_str_part)
  (raw_str_end_part)
] @string

(str_escaped_char) @string.escape

[
  (integer_literal)
  (hex_literal)
  (oct_literal)
  (bin_literal)
  (real_literal)
] @number

(boolean_literal) @boolean
"nil" @constant.builtin

(type_identifier) @type

[
  "func"
  "deinit"
  "protocol"
  "extension"
  "enum"
  "struct"
  "class"
  "typealias"
  "let"
  "var"
  "return"
  "if"
  "else"
  "guard"
  "switch"
  "case"
  "for"
  "in"
  "while"
  "repeat"
  "break"
  "continue"
  "async"
  "await"
  "throw"
  "try"
  "catch"
  "import"
] @keyword

(function_declaration (simple_identifier) @function.method)
(call_expression (simple_identifier) @function.call)

[
  "."
  ";"
  ":"
  ","
] @punctuation.delimiter
