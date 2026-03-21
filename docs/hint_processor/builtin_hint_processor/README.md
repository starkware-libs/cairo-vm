How to run a cairo program with custom hints
====

### Step by Step Guide:

#### Step 1: Add cairo-vm to your project as a crate
Add cairo-vm as a dependency to your Cargo.toml

By using either path:

```rust
[dependencies]
cairo-vm =  {path = "[path to cairo-vm directory"}
```

Or by github link:
```rust
[dependencies]
cairo-vm =  {git = "https://github.com/starkware-libs/cairo-vm.git"}
```

#### Step 2: Code the implementation of your custom hint (Using the helpers and structures described in the sections below)
For this step, you will have to code your hint implementation as a Rust function, and then wrap it inside a Box smart pointer, and a HintFunc (type alias for hint functions).

**Note**: Passing your function as a closure to the Box smart pointer inside HintFunc works too.

The hint implementation must also follow a specific structure in terms of variable input and output:
```rust
fn hint_func(
    vm: &mut VirtualMachine,
    exec_scopes: &mut ExecutionScopes,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
    constants: &HashMap<String, Felt252>,
) -> Result<(), HintError> {
    // Your implementation
}

let hint = HintFunc(Box::new(hint_func));
```

For example, this function implements the hint "print(ids.a)":

```rust
fn print_a_hint(
    vm: &mut VirtualMachine,
    exec_scopes: &mut ExecutionScopes,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
    constants: &HashMap<String, Felt252>,
) -> Result<(), HintError> {
    let a = get_integer_from_var_name("a", vm, ids_data, ap_tracking)?;
    println!("{}", a);
    Ok(())
}

let hint = HintFunc(Box::new(print_a_hint));
```

#### Step 3: Instantiate the BuiltinHintProcessor and add your custom hint implementation
Import the BuiltinHintProcessor from cairo-vm, instantiate it using the `new_empty()` method and the add your custom hint implementation using the method `add_hint`
```rust
use cairo_vm::hint_processor::builtin_hint_processor::builtin_hint_processor_definition::BuiltinHintProcessor
use std::rc::Rc;

let mut hint_processor = BuiltinHintProcessor::new_empty();
hint_processor.add_hint(String::from("print(ids.a)"), Rc::new(hint));
```
You can also create a dictionary of `Rc<HintFunc>` and use the method `new()` to create a `BuiltinHintProcessor` with a preset dictionary of functions instead of using `add_hint()` for each custom hint. Note that `new()` also requires a `RunResources` instance:
```rust
use cairo_vm::vm::runners::cairo_runner::RunResources;
use std::collections::HashMap;
use std::rc::Rc;

let mut map: HashMap<String, Rc<HintFunc>> = HashMap::new();
map.insert(String::from("print(ids.a)"), Rc::new(hint));
let hint_processor = BuiltinHintProcessor::new(map, RunResources::default());
```

#### Step 4: Run your cairo program using BuiltinHintProcessor extended with your hint
Import the function cairo_run from cairo-vm, and run your compiled program

```rust
use cairo_vm::cairo_run::{cairo_run, Cairo0RunConfig};
use cairo_vm::types::layout_name::LayoutName;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

let file = File::open(Path::new("custom_hint.json")).expect("Couldn't load file");
let mut reader = BufReader::new(file);
let mut buffer = Vec::<u8>::new();
reader.read_to_end(&mut buffer).expect("Couldn't read file");

cairo_run(
        &buffer,
        &Cairo0RunConfig {
            layout: LayoutName::all_cairo,
            ..Default::default()
        },
        &mut hint_processor,
    )
    .expect("Couldn't run program");
```
#### Final notes:
The example used in this guide can be found [here](../../../examples/custom_hint/).
The example can be ran using `make example`

### How to code your hint implementation:
In order to code your custom hints you need to take into account the accessible data and the existing helpers.

### Data which can be accessed by hint functions:
* Hint function arguments:
  * `exec_scopes` is the way to interact with the execution scopes in the VM and share data between hints without inserting them into the Cairo execution. It provides methods to create and remove scopes and to modify the current scope, along with several helper methods to allow inserting and retrieving variables of specific types.
  * `vm` is passed in order to give access to the internal state of the VM. It provides mutable references to the memory, memory segment manager and the run context, and immutable references to the builtin runners and the program's prime.
  * `constants`: A dictionary mapping constant's paths to its values. Used to access constants defined in Cairo code.
  * `ap_tracking`: Ap-tracking data of the hint.
  * `ids_data`: A dictionary mapping ids names to their references in the VM's memory. This lets the Rust hint implementation a way to interact with variables defined in Cairo code.

These last two structures are used by helper functions to manage variables from the Cairo scope, and can be overlooked when coding your custom hint implementations. Just note that will have to be passed as arguments to some helper functions.

### Helper functions
There are many helper functions available [here](../../../vm/src/hint_processor/builtin_hint_processor/hint_utils.rs), that will allow you to easily manage cairo variables:

* **get_integer_from_var_name**: gets the value from memory of a integer variable.
* **get_ptr_from_var_name**: gets the value from memory of a pointer variable. 
* **get_address_from_var_name**: gets the address of a given variable as a MaybeRelocatable.
* **get_relocatable_from_var_name**: gets the address of a given variable as a Relocatable.
* **get_maybe_relocatable_from_var_name**: gets the value from memory of a variable as a MaybeRelocatable.
* **get_constant_from_var_name**: gets the value of a constant.
* **get_reference_from_var_name**: gets the value of a var name as a HintReference.
* **insert_value_from_var_name**: assigns a value to a Cairo variable.
* **insert_value_into_ap**: inserts a value to the memory cell pointed to by the ap register.

These methods take the name of the ids variable along with vm, ids_data and ap_tracking.

Note: When handling pointer type variables, computing the address and using it to get the variable from memory might not lead to the correct value (as the variable reference may contain an immediate value that has to be added to the ptr itself), so using the functiom `get_ptr_from_var_name` is strongly recommended.

Note: Cairo's memory is write-once, read-only, so when using `insert_value_from_var_name` its important to first make sure that the variable doesnt contain any value (for example, it may be defined as local but never written) to avoid inconsistent memory errors.

There are also some helpers that dont depend on the hint processor used that can also be used to simplify coding hints [here](../../../vm/src/hint_processor/hint_processor_utils.rs):

* felt_to_usize
* felt_to_u32

You can also find plenty of example implementations in the [builtin hint processor folder](../../../vm/src/hint_processor/builtin_hint_processor).

### Error Handling
This API uses `HintError` as error return type for hint functions. To return a custom error, use `HintError::CustomHint`, which receives a string and prints an error message with the format: "Hint Error: [your message]".
For example, if we want our hint to return an error if `ids.a` is less than 0 we could write:

```rust
if get_integer_from_var_name("a", vm, ids_data, ap_tracking)? < 0 {
  return Err(HintError::CustomHint("a < 0".into()));
}
```
