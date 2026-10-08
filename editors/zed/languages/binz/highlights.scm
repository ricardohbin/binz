; binZ syntax highlighting. Later patterns win in Zed, so the specific ones
; come after the general ones.

(identifier) @variable

(parameter name: (identifier) @variable.parameter)

(field_identifier) @property

; ---------------------------------------------------------------- types

(type_identifier) @type
(primitive_type) @type.builtin
(container_keyword) @type.builtin
(map_keyword) @type.builtin
(tuple_keyword) @type.builtin

(struct_literal name: (type_identifier) @constructor)

; ------------------------------------------------------------- functions

(function_definition name: (identifier) @function)

(call_expression function: (identifier) @function)
(call_expression
  function: (field_expression field: (field_identifier) @function))

(stub_definition
  module: (identifier) @namespace
  name: (field_identifier) @function)

; --------------------------------------------------------------- modules

(module_path (identifier) @namespace)
(root_path (identifier) @namespace)
(path_anchor) @namespace
(path_extension) @namespace
(import_declaration alias: (identifier) @namespace)

; ----------------------------------------------------------- annotations

(annotation "@" @attribute name: (identifier) @attribute)

; -------------------------------------------------------------- literals

(integer_literal) @number
(float_literal) @number
(boolean_literal) @boolean
(string) @string
(escape_sequence) @string.escape

(line_comment) @comment
(block_comment) @comment

; -------------------------------------------------------------- keywords

[
  "import"
  "as"
  "struct"
  "const"
  "var"
  "stub"
] @keyword

"function" @keyword.function

"return" @keyword.return

[
  "if"
  "else"
] @keyword.conditional

"while" @keyword.repeat

"cast" @keyword.operator

; ------------------------------------------------------------- operators

[
  "="
  "=="
  "!="
  "<"
  "<="
  ">"
  ">="
  "+"
  "-"
  "*"
  "/"
  "%"
  "&"
  "&&"
  "||"
  "!"
] @operator

(root_path "@" @namespace)
(module_path "/" @namespace)
(root_path "/" @namespace)
(root_path "." @namespace)

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

[
  ","
  ";"
  ":"
  "."
] @punctuation.delimiter

; Generic angle brackets are brackets, not comparisons.
(container_type ["<" ">"] @punctuation.bracket)
(map_type ["<" ">"] @punctuation.bracket)
(tuple_type ["<" ">"] @punctuation.bracket)
(container_literal ["<" ">"] @punctuation.bracket)
(map_literal ["<" ">"] @punctuation.bracket)
(cast_expression ["<" ">"] @punctuation.bracket)
