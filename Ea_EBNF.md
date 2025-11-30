program          = { declaration } , { rule }

declaration      = input_decl
                 | capability_decl
                 | const_decl
                 | metadata_decl

input_decl       = "input" identifier "<" type ">"
capability_decl  = "capability" identifier "(" [param_list] ")" [ "->" result_type ]
const_decl       = "const" identifier ":" type "=" literal
metadata_decl    = identifier ":" string_literal

rule             = "rule" event_name ":" { statement }

event_name       = "on_boot"
                 | "on_lattice_update(" identifier ":" type ")"
                 | "on_timer_1hz"
                 | "on_self_integrity_failure"
                 | identifier

statement        = verify_stmt
                 | let_stmt
                 | if_stmt
                 | emit_stmt
                 | schedule_stmt
                 | unschedule_stmt
                 | static_decl
                 | expression

verify_stmt      = "verify" expression
let_stmt         = "let" identifier [ "=" expression ]
if_stmt          = "if" expression "->" action [ "else" "->" action ]
emit_stmt        = "emit" identifier "(" [arg_list] ")"
schedule_stmt    = "schedule(" expression "," "priority:" literal ")"
unschedule_stmt  = "unschedule(" "muscle_id:" expression ")"

expression       = literal
                 | identifier
                 | field_access
                 | call_expr
                 | binary_expr
                 | "self.id" | "self.version"

type             = "MuscleUpdate" | "DeviceProof" | "SealedBlob" | "ExecutableMuscle"
                 | "muscle_id" | "u8" | "u64" | "[u8; 32]"

literal          = hex_literal | integer_literal | string_literal
hex_literal      = "0x" [0-9a-fA-F]+
