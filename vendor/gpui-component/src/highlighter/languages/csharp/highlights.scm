; Adapted from tree-sitter-c-sharp's MIT-licensed highlights query.

(comment) @comment

[
  (real_literal)
  (integer_literal)
] @number

[
  (character_literal)
  (string_literal)
  (raw_string_literal)
  (verbatim_string_literal)
  (interpolated_string_expression)
] @string

(escape_sequence) @string.escape

[
  (boolean_literal)
  (null_literal)
] @constant.builtin

(predefined_type) @type.builtin

[
  "class"
  "interface"
  "enum"
  "struct"
  "record"
  "namespace"
  "public"
  "private"
  "protected"
  "internal"
  "static"
  "abstract"
  "sealed"
  "async"
  "await"
  "new"
  "return"
  "if"
  "else"
  "switch"
  "case"
  "for"
  "foreach"
  "while"
  "do"
  "try"
  "catch"
  "finally"
  "throw"
  "using"
  "var"
  "const"
  "readonly"
  "get"
  "set"
] @keyword

(method_declaration name: (identifier) @function)
(class_declaration name: (identifier) @type)
(interface_declaration name: (identifier) @type)
(enum_declaration name: (identifier) @type)
(struct_declaration (identifier) @type)
(namespace_declaration name: (identifier) @module)

[
  ";"
  "."
  ","
] @punctuation.delimiter
