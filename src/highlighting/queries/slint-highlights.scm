;; Slint highlights query
;; Adapted from slint-ui/slint (editors/zed/languages/slint/highlights.scm)
;; Original copyright: Luke D. Jones <luke@ljones.dev>, MIT license

(comment) @comment

(string_value) @string
(escape_sequence) @string.escape
(color_value) @constant

[
  (children_identifier)
  (easing_kind_identifier)
] @constant.builtin

[
  (int_value)
  (physical_length_value)
] @number

[
  (angle_value)
  (duration_value)
  (float_value)
  (length_value)
  (percent_value)
  (relative_font_size_value)
] @number

(purity) @keyword
(function_visibility) @keyword
(property_visibility) @keyword

(builtin_type_identifier) @type.builtin
(reference_identifier) @variable.builtin

(type
  [
    (type_list)
    (user_type_identifier)
    (anon_struct_block)
  ]) @type

(user_type_identifier) @type

(argument) @variable.parameter

(function_call
  name: (_) @function)

(callback
  name: (_) @function)

(callback_alias
  name: (_) @function)

(component
  id: (_) @variable)

(enum_definition
  name: (_) @type)

(function_definition
  name: (_) @function)

(struct_definition
  name: (_) @type)

(typed_identifier
  type: (_) @type)

(binary_expression
  op: (_) @operator)

(unary_expression
  op: (_) @operator)

[
  (comparison_operator)
  (mult_prec_operator)
  (add_prec_operator)
  (unary_prec_operator)
  (assignment_prec_operator)
] @operator

[
  ":="
  "=>"
  "->"
  "<=>"
] @operator

[
  ";"
  "."
  ","
  ":"
] @punctuation.delimiter

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

(property
  [
    "<"
    ">"
  ] @punctuation.bracket)

(component
  id: (simple_identifier) @constant)

(property
  name: (simple_identifier) @property)

(binding_alias
  name: (simple_identifier) @property)

(binding
  name: (simple_identifier) @property)

(struct_block
  (simple_identifier) @property)

(anon_struct_block
  (simple_identifier) @property)

(property_assignment
  property: (simple_identifier) @property)

(states_definition
  name: (simple_identifier) @variable)

(callback
  name: (simple_identifier) @variable)

(typed_identifier
  name: (_) @variable)

(expression
  (simple_identifier) @variable)

(member_access
  member:
    (expression
      (simple_identifier) @property))

(states_definition
  name: (simple_identifier) @constant)

[
  (linear_gradient_identifier)
  (radial_gradient_identifier)
  (radial_gradient_kind)
] @attribute

(image_call
  "@image-url" @attribute)

(tr
  "@tr" @attribute)

(animate_option_identifier) @keyword
(export) @keyword

(if_statement
  "if" @keyword)

(if_expr
  [
    "if"
    "else"
  ] @keyword)

(animate_statement
  "animate" @keyword)

(callback
  "callback" @keyword)

(component_definition
  [
    "component"
    "inherits"
  ] @keyword)

(enum_definition
  "enum" @keyword)

(for_loop
  [
    "for"
    "in"
  ] @keyword)

(function_definition
  "function" @keyword)

(global_definition
  "global" @keyword)

(imperative_block
  "return" @keyword)

(import_statement
  [
    "import"
    "from"
  ] @keyword)

(import_type
  "as" @keyword)

(property
  "property" @keyword)

(states_definition
  [
    "states"
    "when"
  ] @keyword)

(struct_definition
  "struct" @keyword)

(transitions_definition
  [
    "transitions"
    "in"
    "out"
  ] @keyword)
