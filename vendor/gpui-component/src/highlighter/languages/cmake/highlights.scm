; Adapted from tree-sitter-cmake's MIT-licensed highlights query.

[
  (quoted_argument)
  (bracket_argument)
] @string

[
  (bracket_comment)
  (line_comment)
] @comment

(variable) @variable

(normal_command (identifier) @function)

[
  (function)
  (endfunction)
  (macro)
  (endmacro)
] @keyword.function

[
  (if)
  (elseif)
  (else)
  (endif)
] @keyword.conditional

[
  (foreach)
  (endforeach)
  (while)
  (endwhile)
] @keyword.repeat

[
  "ENV"
  "CACHE"
] @module

[
  "$"
  "{"
  "}"
] @punctuation.special

[
  "("
  ")"
] @punctuation.bracket
