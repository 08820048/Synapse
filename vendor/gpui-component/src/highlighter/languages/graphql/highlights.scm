; Core GraphQL syntax captured against tree-sitter-graphql.

(comment) @comment
(string_value) @string
(int_value) @number
(float_value) @number
(boolean_value) @boolean
(null_value) @constant.builtin

[
  "query"
  "mutation"
  "subscription"
  "fragment"
  "on"
  "schema"
  "scalar"
  "type"
  "interface"
  "union"
  "enum"
  "input"
  "directive"
  "extend"
  "implements"
  "repeatable"
] @keyword

(named_type (name) @type)
(directive (name) @attribute)
