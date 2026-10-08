/**
 * Tree-sitter grammar for binZ.
 *
 * Mirrors `src/lexer.rs` and `src/parser.rs`: it accepts the shapes the
 * parser accepts and leaves every rule the compiler enforces (imports first,
 * `@field` only inside `@json`, test names, ...) to the compiler. An editor
 * grammar has to keep highlighting a file while it is half typed, so it is
 * deliberately looser than `binz` itself.
 */

const PREC = {
  or: 1,
  and: 2,
  equality: 3,
  compare: 4,
  additive: 5,
  multiplicative: 6,
  unary: 7,
  postfix: 8,
};

const typeIdentifier = ($) => alias($.identifier, $.type_identifier);
const fieldIdentifier = ($) => alias($.identifier, $.field_identifier);

const commaSep = (rule) => optional(seq(rule, repeat(seq(',', rule)), optional(',')));

module.exports = grammar({
  name: 'binz',

  word: ($) => $.identifier,

  extras: ($) => [/\s/, $.line_comment, $.block_comment],

  supertypes: ($) => [$._type, $._expression, $._statement],

  rules: {
    source_file: ($) =>
      repeat(choice($.import_declaration, $.struct_definition, $.function_definition)),

    // ------------------------------------------------------------ imports

    import_declaration: ($) =>
      seq(
        'import',
        field('path', choice($.module_path, $.root_path)),
        optional(seq('as', field('alias', $.identifier))),
        ';',
      ),

    // `binz/io`
    module_path: ($) => seq($.identifier, repeat(seq('/', $.identifier))),

    // `@root/utils/math.binz`
    root_path: ($) =>
      seq(
        '@',
        alias('root', $.path_anchor),
        repeat1(seq('/', $.identifier)),
        '.',
        alias('binz', $.path_extension),
      ),

    // `@test`, `@json`, `@field("key")`
    annotation: ($) =>
      seq('@', field('name', $.identifier), optional(seq('(', field('argument', $.string), ')'))),

    // -------------------------------------------------------------- items

    struct_definition: ($) =>
      seq(
        repeat($.annotation),
        'struct',
        field('name', typeIdentifier($)),
        field('body', $.field_declaration_list),
      ),

    field_declaration_list: ($) => seq('{', commaSep($.field_declaration), '}'),

    field_declaration: ($) =>
      seq(repeat($.annotation), field('name', fieldIdentifier($)), ':', field('type', $._type)),

    function_definition: ($) =>
      seq(
        repeat($.annotation),
        'function',
        field('name', $.identifier),
        field('parameters', $.parameter_list),
        ':',
        field('return_type', $._type),
        field('body', $.block),
      ),

    parameter_list: ($) => seq('(', commaSep($.parameter), ')'),

    parameter: ($) => seq(field('name', $.identifier), ':', field('type', $._type)),

    // -------------------------------------------------------------- types

    _type: ($) =>
      choice(
        $.primitive_type,
        typeIdentifier($),
        $.array_type,
        $.container_type,
        $.map_type,
        $.tuple_type,
        $.pointer_type,
        $.function_type,
      ),

    primitive_type: (_) => choice('i32', 'i64', 'f64', 'bool', 'str', 'void'),

    container_keyword: (_) => choice('Vector', 'LinkedList', 'Set', 'SortedSet'),

    map_keyword: (_) => choice('HashMap', 'SortedMap'),

    array_type: ($) =>
      seq('[', field('element', $._type), ';', field('length', $.integer_literal), ']'),

    container_type: ($) =>
      seq(field('kind', $.container_keyword), '<', field('element', $._type), '>'),

    map_type: ($) =>
      seq(
        field('kind', $.map_keyword),
        '<',
        field('key', $._type),
        ',',
        field('value', $._type),
        '>',
      ),

    // `Tuple<Error, i32>`
    tuple_type: ($) =>
      seq(field('kind', $.tuple_keyword), '<', $._type, repeat1(seq(',', $._type)), '>'),

    tuple_keyword: (_) => 'Tuple',

    pointer_type: ($) => prec.right(seq('*', field('pointee', $._type))),

    function_type: ($) =>
      prec.right(
        seq(
          'function',
          '(',
          commaSep($._type),
          ')',
          ':',
          field('return_type', $._type),
        ),
      ),

    // --------------------------------------------------------- statements

    block: ($) => seq('{', repeat($._statement), '}'),

    _statement: ($) =>
      choice(
        $.variable_declaration,
        $.return_statement,
        $.if_statement,
        $.while_statement,
        $.stub_definition,
        $.assignment_statement,
        $.expression_statement,
        $.block,
      ),

    variable_declaration: ($) =>
      seq(
        choice('const', 'var'),
        field('name', $.identifier),
        ':',
        field('type', $._type),
        '=',
        field('value', $._expression),
        ';',
      ),

    return_statement: ($) => seq('return', optional($._expression), ';'),

    if_statement: ($) =>
      seq(
        'if',
        field('condition', $.parenthesized_expression),
        field('consequence', $.block),
        optional(seq('else', field('alternative', choice($.block, $.if_statement)))),
      ),

    while_statement: ($) =>
      seq('while', field('condition', $.parenthesized_expression), field('body', $.block)),

    // `stub rates.lookup(country: str): f64 { ... }`
    stub_definition: ($) =>
      seq(
        'stub',
        field('module', $.identifier),
        '.',
        field('name', fieldIdentifier($)),
        field('parameters', $.parameter_list),
        ':',
        field('return_type', $._type),
        field('body', $.block),
      ),

    assignment_statement: ($) =>
      seq(field('left', $._expression), '=', field('right', $._expression), ';'),

    expression_statement: ($) => seq($._expression, ';'),

    // -------------------------------------------------------- expressions

    _expression: ($) =>
      choice(
        $.identifier,
        $.integer_literal,
        $.float_literal,
        $.string,
        $.boolean_literal,
        $.parenthesized_expression,
        $.array_literal,
        $.array_repeat,
        $.container_literal,
        $.map_literal,
        $.tuple_literal,
        $.struct_literal,
        $.cast_expression,
        $.unary_expression,
        $.binary_expression,
        $.call_expression,
        $.index_expression,
        $.field_expression,
      ),

    parenthesized_expression: ($) => seq('(', $._expression, ')'),

    binary_expression: ($) => {
      const table = [
        [PREC.or, '||'],
        [PREC.and, '&&'],
        [PREC.equality, choice('==', '!=')],
        [PREC.compare, choice('<', '<=', '>', '>=')],
        [PREC.additive, choice('+', '-')],
        [PREC.multiplicative, choice('*', '/', '%')],
      ];
      return choice(
        ...table.map(([p, op]) =>
          prec.left(
            p,
            seq(field('left', $._expression), field('operator', op), field('right', $._expression)),
          ),
        ),
      );
    },

    unary_expression: ($) =>
      prec(
        PREC.unary,
        seq(field('operator', choice('-', '!', '*', '&')), field('operand', $._expression)),
      ),

    call_expression: ($) =>
      prec(
        PREC.postfix,
        seq(field('function', $._expression), field('arguments', $.argument_list)),
      ),

    argument_list: ($) => seq('(', commaSep($._expression), ')'),

    index_expression: ($) =>
      prec(PREC.postfix, seq(field('value', $._expression), '[', field('index', $._expression), ']')),

    field_expression: ($) =>
      prec(PREC.postfix, seq(field('value', $._expression), '.', field('field', fieldIdentifier($)))),

    cast_expression: ($) =>
      seq('cast', '<', field('type', $._type), '>', '(', field('value', $._expression), ')'),

    // `[1, 2, 3]`
    array_literal: ($) =>
      seq('[', $._expression, repeat(seq(',', $._expression)), optional(','), ']'),

    // `[0; 64]`
    array_repeat: ($) =>
      seq('[', field('value', $._expression), ';', field('count', $.integer_literal), ']'),

    // `Vector<i32>{1, 2, 3}`
    container_literal: ($) =>
      seq(
        field('kind', $.container_keyword),
        '<',
        field('element', $._type),
        '>',
        '{',
        commaSep($._expression),
        '}',
      ),

    // `HashMap<str, i32>{"a": 1}`
    map_literal: ($) =>
      seq(
        field('kind', $.map_keyword),
        '<',
        field('key', $._type),
        ',',
        field('value', $._type),
        '>',
        '{',
        commaSep($.map_entry),
        '}',
      ),

    // `Tuple<Error, i32>(err, 42)`
    tuple_literal: ($) =>
      seq(field('type', $.tuple_type), '(', commaSep($._expression), ')'),

    map_entry: ($) => seq(field('key', $._expression), ':', field('value', $._expression)),

    // `Point { x: 1, y: 2 }` and `Point{}`
    struct_literal: ($) =>
      seq(field('name', typeIdentifier($)), '{', commaSep($.field_initializer), '}'),

    field_initializer: ($) =>
      seq(field('name', fieldIdentifier($)), ':', field('value', $._expression)),

    // ------------------------------------------------------------ lexemes

    identifier: (_) => /[A-Za-z_][A-Za-z0-9_]*/,

    integer_literal: (_) => /\d+/,

    float_literal: (_) => /\d+\.\d+/,

    boolean_literal: (_) => choice('true', 'false'),

    string: ($) =>
      seq('"', repeat(choice($.string_content, $.escape_sequence)), token.immediate('"')),

    string_content: (_) => token.immediate(prec(1, /[^"\\\n]+/)),

    escape_sequence: (_) => token.immediate(/\\[ntr0\\"]/),

    line_comment: (_) => token(seq('//', /[^\n]*/)),

    block_comment: (_) => token(seq('/*', /[^*]*\*+([^/*][^*]*\*+)*/, '/')),
  },
});
