; What the graph reads from Rust. The captures are the roles of `languages/mod.rs`.
; Strings and comments are never captured.

; Definitions.
(function_item name: (identifier) @name) @def.function
(function_signature_item name: (identifier) @name) @def.function
(macro_definition name: (identifier) @name) @def.function
(struct_item name: (type_identifier) @name) @def.type
(enum_item name: (type_identifier) @name) @def.type
(union_item name: (type_identifier) @name) @def.type
(type_item name: (type_identifier) @name) @def.type
(trait_item name: (type_identifier) @name) @def.trait
(const_item name: (identifier) @name) @def.constant
(static_item name: (identifier) @name) @def.constant
(mod_item name: (identifier) @name) @def.module

; A test: a function under an attribute whose last name is `test` (`#[test]`,
; `#[tokio::test]`), whatever other attributes and comments stand between them.
((attribute_item
   (attribute [(identifier) @_mark (scoped_identifier name: (identifier) @_mark)]))
 . [(attribute_item) (line_comment) (block_comment)]*
 . (function_item name: (identifier) @name) @def.test
 (#eq? @_mark "test"))

; Scopes. An impl of a type the patterns below cannot name is still a scope, without
; a name: its functions are methods with no parent.
(impl_item body: (declaration_list) @scope.type)
(impl_item
  type: [(type_identifier) @scope.name
         (scoped_type_identifier name: (type_identifier) @scope.name)
         (generic_type
           type: [(type_identifier) @scope.name
                  (scoped_type_identifier name: (type_identifier) @scope.name)])]
  body: (declaration_list) @scope.type)
(trait_item name: (type_identifier) @scope.name body: (declaration_list) @scope.type)
(mod_item name: (identifier) @scope.name body: (declaration_list) @scope.module)
(function_item body: (block) @scope.code)

; Uses: call targets, type names, the parts of a path, macro and attribute names, and
; bare names that start upper case.
(call_expression
  function: [(identifier) @use
             (field_expression field: (field_identifier) @use)
             (generic_function
               function: [(identifier) @use
                          (field_expression field: (field_identifier) @use)])])
((type_identifier) @use (#not-eq? @use "Self"))
(scoped_identifier name: (identifier) @use)
(macro_invocation macro: (identifier) @use)
(attribute (identifier) @use)
((identifier) @use (#match? @use "^\\p{Lu}") (#not-eq? @use "Self"))
; The arguments of a macro are tokens, not syntax: a name right before `(...)` is a call.
(token_tree (identifier) @use . (token_tree . "("))

; Imports, split into names by `languages/rust.rs`.
(use_declaration) @import
(extern_crate_declaration) @import

; What the parser could not read; see `Spec::quotes`.
(ERROR) @error
