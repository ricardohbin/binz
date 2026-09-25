(struct_definition
  "struct" @context
  name: (type_identifier) @name) @item

(field_declaration
  name: (field_identifier) @name) @item

(function_definition
  "function" @context
  name: (identifier) @name) @item

(stub_definition
  "stub" @context
  module: (identifier) @name
  "." @name
  name: (field_identifier) @name) @item
