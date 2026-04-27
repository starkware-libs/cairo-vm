use std::{any::Any, cell::RefCell, collections::HashMap, rc::Rc};

use crate::{
    types::{exec_scope::ExecutionScopes, relocatable::MaybeRelocatable},
    vm::{errors::hint_errors::HintError, vm_core::VirtualMachine},
};

use crate::{
    any_box,
    hint_processor::{
        builtin_hint_processor::hint_utils::{
            get_ptr_from_var_name, insert_value_from_var_name, insert_value_into_ap,
        },
        hint_processor_definition::HintReference,
    },
    serde::deserialize_program::ApTracking,
};

use super::{dict_manager::DictManager, hint_utils::get_maybe_relocatable_from_var_name};

//DictAccess struct has three memebers, so the size of DictAccess* is 3
pub const DICT_ACCESS_SIZE: usize = 3;

fn copy_initial_dict(
    exec_scopes: &mut ExecutionScopes,
) -> Option<HashMap<MaybeRelocatable, MaybeRelocatable>> {
    let mut initial_dict: Option<HashMap<MaybeRelocatable, MaybeRelocatable>> = None;
    if let Some(variable) = exec_scopes.get_local_variables().ok()?.get("initial_dict") {
        if let Some(dict) = variable.downcast_ref::<HashMap<MaybeRelocatable, MaybeRelocatable>>() {
            initial_dict = Some(dict.clone());
        }
    }
    initial_dict
}

/*Implements hint:
   if '__dict_manager' not in globals():
           from starkware.cairo.common.dict import DictManager
           __dict_manager = DictManager()

       memory[ap] = __dict_manager.new_dict(segments, initial_dict)
       del initial_dict

For now, the functionality to create a dictionary from a previously defined initial_dict (using a hint)
is not available
*/
pub fn dict_new(
    vm: &mut VirtualMachine,
    exec_scopes: &mut ExecutionScopes,
) -> Result<(), HintError> {
    //Get initial dictionary from scope (defined by an earlier hint)
    let initial_dict = copy_initial_dict(exec_scopes).ok_or(HintError::NoInitialDict)?;
    //Check if there is a dict manager in scope, create it if there isnt one
    let base = if let Ok(dict_manager) = exec_scopes.get_dict_manager() {
        dict_manager.borrow_mut().new_dict(vm, initial_dict)?
    } else {
        let mut dict_manager = DictManager::new();
        let base = dict_manager.new_dict(vm, initial_dict)?;
        exec_scopes.insert_value("dict_manager", Rc::new(RefCell::new(dict_manager)));
        base
    };
    insert_value_into_ap(vm, base)
}

/*Implements hint:
   if '__dict_manager' not in globals():
            from starkware.cairo.common.dict import DictManager
            __dict_manager = DictManager()

        memory[ap] = __dict_manager.new_default_dict(segments, ids.default_value)

For now, the functionality to create a dictionary from a previously defined initial_dict (using a hint)
is not available, an empty dict is created always
*/
pub fn default_dict_new(
    vm: &mut VirtualMachine,
    exec_scopes: &mut ExecutionScopes,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
) -> Result<(), HintError> {
    //Check that ids contains the reference id for each variable used by the hint
    let default_value =
        get_maybe_relocatable_from_var_name("default_value", vm, ids_data, ap_tracking)?;
    //Get initial dictionary from scope (defined by an earlier hint) if available
    let initial_dict = copy_initial_dict(exec_scopes);
    //Check if there is a dict manager in scope, create it if there isnt one
    let base = if let Ok(dict_manager) = exec_scopes.get_dict_manager() {
        dict_manager
            .borrow_mut()
            .new_default_dict(vm, &default_value, initial_dict)?
    } else {
        let mut dict_manager = DictManager::new();
        let base = dict_manager.new_default_dict(vm, &default_value, initial_dict)?;
        exec_scopes.insert_value("dict_manager", Rc::new(RefCell::new(dict_manager)));
        base
    };
    insert_value_into_ap(vm, base)
}

/* Implements hint:
   dict_tracker = __dict_manager.get_tracker(ids.dict_ptr)
   dict_tracker.current_ptr += ids.DictAccess.SIZE
   ids.value = dict_tracker.data[ids.key]
*/
pub fn dict_read(
    vm: &mut VirtualMachine,
    exec_scopes: &mut ExecutionScopes,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
) -> Result<(), HintError> {
    let key = get_maybe_relocatable_from_var_name("key", vm, ids_data, ap_tracking)?;
    let dict_ptr = get_ptr_from_var_name("dict_ptr", vm, ids_data, ap_tracking)?;
    let dict_manager_ref = exec_scopes.get_dict_manager()?;
    let mut dict = dict_manager_ref.borrow_mut();
    let tracker = dict.get_tracker_mut(dict_ptr)?;
    tracker.current_ptr.offset += DICT_ACCESS_SIZE;
    let value = tracker.get_value(&key)?;
    insert_value_from_var_name("value", value.clone(), vm, ids_data, ap_tracking)
}

/* Implements hint:
    dict_tracker = __dict_manager.get_tracker(ids.dict_ptr)
    dict_tracker.current_ptr += ids.DictAccess.SIZE
    ids.dict_ptr.prev_value = dict_tracker.data[ids.key]
    dict_tracker.data[ids.key] = ids.new_value
*/
pub fn dict_write(
    vm: &mut VirtualMachine,
    exec_scopes: &mut ExecutionScopes,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
) -> Result<(), HintError> {
    let key = get_maybe_relocatable_from_var_name("key", vm, ids_data, ap_tracking)?;
    let new_value = get_maybe_relocatable_from_var_name("new_value", vm, ids_data, ap_tracking)?;
    let dict_ptr = get_ptr_from_var_name("dict_ptr", vm, ids_data, ap_tracking)?;
    //Get tracker for dictionary
    let dict_manager_ref = exec_scopes.get_dict_manager()?;
    let mut dict = dict_manager_ref.borrow_mut();
    let tracker = dict.get_tracker_mut(dict_ptr)?;
    //dict_ptr is a pointer to a struct, with the ordered fields (key, prev_value, new_value),
    //dict_ptr.prev_value will be equal to dict_ptr + 1
    let dict_ptr_prev_value = (dict_ptr + 1_i32)?;
    //Tracker set to track next dictionary entry
    tracker.current_ptr.offset += DICT_ACCESS_SIZE;
    //Get previous value
    let prev_value = tracker.get_value(&key)?.clone();
    //Insert new value into tracker
    tracker.insert_value(&key, &new_value);
    //Insert previous value into dict_ptr.prev_value
    //Addres for dict_ptr.prev_value should be dict_ptr* + 1 (defined above)
    vm.insert_value(dict_ptr_prev_value, prev_value)?;
    Ok(())
}

/* Implements hint:
    # Verify dict pointer and prev value.
        dict_tracker = __dict_manager.get_tracker(ids.dict_ptr)
        current_value = dict_tracker.data[ids.key]
        assert current_value == ids.prev_value, \
            f'Wrong previous value in dict. Got {ids.prev_value}, expected {current_value}.'

        # Update value.
        dict_tracker.data[ids.key] = ids.new_value
        dict_tracker.current_ptr += ids.DictAccess.SIZE
*/
pub fn dict_update(
    vm: &mut VirtualMachine,
    exec_scopes: &mut ExecutionScopes,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
) -> Result<(), HintError> {
    let key = get_maybe_relocatable_from_var_name("key", vm, ids_data, ap_tracking)?;
    let prev_value = get_maybe_relocatable_from_var_name("prev_value", vm, ids_data, ap_tracking)?;
    let new_value = get_maybe_relocatable_from_var_name("new_value", vm, ids_data, ap_tracking)?;
    let dict_ptr = get_ptr_from_var_name("dict_ptr", vm, ids_data, ap_tracking)?;

    //Get tracker for dictionary
    let dict_manager_ref = exec_scopes.get_dict_manager()?;
    let mut dict = dict_manager_ref.borrow_mut();
    let tracker = dict.get_tracker_mut(dict_ptr)?;
    //Check that prev_value is equal to the current value at the given key
    let current_value = tracker.get_value(&key)?;
    if current_value != &prev_value {
        return Err(HintError::WrongPrevValue(Box::new((
            prev_value,
            current_value.into(),
            key,
        ))));
    }
    //Update Value
    tracker.insert_value(&key, &new_value);
    tracker.current_ptr.offset += DICT_ACCESS_SIZE;
    Ok(())
}

/* Implements hint:
   # Prepare arguments for dict_new. In particular, the same dictionary values should be copied
   # to the new (squashed) dictionary.
   vm_enter_scope({
       # Make __dict_manager accessible.
       '__dict_manager': __dict_manager,
       # Create a copy of the dict, in case it changes in the future.
       'initial_dict': dict(__dict_manager.get_dict(ids.dict_accesses_end)),
   })
*/
pub fn dict_squash_copy_dict(
    vm: &mut VirtualMachine,
    exec_scopes: &mut ExecutionScopes,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
) -> Result<(), HintError> {
    let dict_accesses_end = get_ptr_from_var_name("dict_accesses_end", vm, ids_data, ap_tracking)?;
    let dict_manager_ref = exec_scopes.get_dict_manager()?;
    let dict_manager = dict_manager_ref.borrow();
    let dict_copy: Box<dyn Any> = Box::new(
        dict_manager
            .get_tracker(dict_accesses_end)?
            .get_dictionary_copy(),
    );
    exec_scopes.enter_scope(HashMap::from([
        (
            String::from("dict_manager"),
            any_box!(exec_scopes.get_dict_manager()?),
        ),
        (String::from("initial_dict"), dict_copy),
    ]));
    Ok(())
}

/* Implements Hint:
    # Update the DictTracker's current_ptr to point to the end of the squashed dict.
    __dict_manager.get_tracker(ids.squashed_dict_start).current_ptr = \
    ids.squashed_dict_end.address_
*/
pub fn dict_squash_update_ptr(
    vm: &mut VirtualMachine,
    exec_scopes: &mut ExecutionScopes,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
) -> Result<(), HintError> {
    let squashed_dict_start =
        get_ptr_from_var_name("squashed_dict_start", vm, ids_data, ap_tracking)?;
    let squashed_dict_end = get_ptr_from_var_name("squashed_dict_end", vm, ids_data, ap_tracking)?;
    exec_scopes
        .get_dict_manager()?
        .borrow_mut()
        .get_tracker_mut(squashed_dict_start)?
        .current_ptr = squashed_dict_end;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::any_box;
    use crate::hint_processor::builtin_hint_processor::builtin_hint_processor_definition::BuiltinHintProcessor;
    use crate::hint_processor::builtin_hint_processor::builtin_hint_processor_definition::HintProcessorData;
    use crate::hint_processor::builtin_hint_processor::dict_manager::Dictionary;
    use crate::hint_processor::builtin_hint_processor::hint_code;
    use crate::hint_processor::hint_processor_definition::HintProcessorLogic;
    use crate::types::exec_scope::ExecutionScopes;
    use std::collections::HashMap;

    use crate::{
        hint_processor::builtin_hint_processor::dict_manager::{DictManager, DictTracker},
        relocatable,
        types::relocatable::{MaybeRelocatable, Relocatable},
        utils::test_utils::*,
        vm::errors::memory_errors::MemoryError,
    };
    use assert_matches::assert_matches;

    #[test]
    fn run_dict_new_with_initial_dict_empty() {
        let hint_code = "if '__dict_manager' not in globals():\n    from starkware.cairo.common.dict import DictManager\n    __dict_manager = DictManager()\n\nmemory[ap] = __dict_manager.new_dict(segments, initial_dict)\ndel initial_dict";
        let mut vm = vm!();
        add_segments!(vm, 1);

        //Store initial dict in scope
        let mut exec_scopes = scope![(
            "initial_dict",
            HashMap::<MaybeRelocatable, MaybeRelocatable>::new()
        )];
        //ids and references are not needed for this test
        run_hint!(vm, HashMap::new(), hint_code, &mut exec_scopes)
            .expect("Error while executing hint");
        //first new segment is added for the dictionary
        assert_eq!(vm.segments.num_segments(), 2);
        //new segment base (1,0) is inserted into ap (1,0)
        check_memory![vm.segments.memory, ((1, 0), (1, 0))];
        //Check the dict manager has a tracker for segment 0,
        //and that tracker contains the ptr (1,0) and an empty dict
        assert_eq!(
            exec_scopes
                .get_dict_manager()
                .unwrap()
                .borrow()
                .trackers
                .get(&1),
            Some(&DictTracker::new_empty(relocatable!(1, 0)))
        );
    }

    #[test]
    fn run_dict_new_with_no_initial_dict() {
        let hint_code = "if '__dict_manager' not in globals():\n    from starkware.cairo.common.dict import DictManager\n    __dict_manager = DictManager()\n\nmemory[ap] = __dict_manager.new_dict(segments, initial_dict)\ndel initial_dict";
        let mut vm = vm!();
        //ids and references are not needed for this test
        assert_matches!(
            run_hint!(vm, HashMap::new(), hint_code),
            Err(HintError::NoInitialDict)
        );
    }

    #[test]
    fn run_dict_new_ap_is_taken() {
        let hint_code = "if '__dict_manager' not in globals():\n    from starkware.cairo.common.dict import DictManager\n    __dict_manager = DictManager()\n\nmemory[ap] = __dict_manager.new_dict(segments, initial_dict)\ndel initial_dict";
        let mut vm = vm!();
        let mut exec_scopes = scope![(
            "initial_dict",
            HashMap::<MaybeRelocatable, MaybeRelocatable>::new()
        )];
        vm.segments = segments![((1, 0), 1)];
        //ids and references are not needed for this test
        assert_matches!(
            run_hint!(vm, HashMap::new(), hint_code, &mut exec_scopes),
            Err(HintError::Memory(
                MemoryError::InconsistentMemory(bx)
            )) if *bx == (Relocatable::from((1, 0)),
                    MaybeRelocatable::from(1),
                    MaybeRelocatable::from((2, 0)))
        );
    }

    #[test]
    fn run_dict_read_valid() {
        let hint_code = "dict_tracker = __dict_manager.get_tracker(ids.dict_ptr)\ndict_tracker.current_ptr += ids.DictAccess.SIZE\nids.value = dict_tracker.data[ids.key]";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 3;
        //Insert ids into memory
        vm.segments = segments![((1, 0), 5), ((1, 2), (2, 0))];
        let ids_data = ids_data!["key", "value", "dict_ptr"];
        add_segments!(vm, 1);
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager!(&mut exec_scopes, 2, (5, 12));
        //Execute the hint
        assert_matches!(run_hint!(vm, ids_data, hint_code, &mut exec_scopes), Ok(()));
        //Check that value variable (at address (1,1)) contains the proper value
        assert_eq!(
            vm.segments
                .memory
                .get(&MaybeRelocatable::from((1, 1)))
                .unwrap(),
            MaybeRelocatable::from(12)
        );
        //Check that the tracker's current_ptr has moved accordingly
        check_dict_ptr!(&exec_scopes, 2, (2, 3));
    }

    #[test]
    fn run_dict_read_invalid_key() {
        let hint_code = "dict_tracker = __dict_manager.get_tracker(ids.dict_ptr)\ndict_tracker.current_ptr += ids.DictAccess.SIZE\nids.value = dict_tracker.data[ids.key]";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 3;
        //Insert ids into memory
        vm.segments = segments![((1, 0), 6), ((1, 2), (2, 0))];
        let ids_data = ids_data!["key", "value", "dict_ptr"];
        //Execute the hint
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager!(&mut exec_scopes, 2, (5, 12));
        assert_matches!(
            run_hint!(vm, ids_data, hint_code, &mut exec_scopes),
            Err(HintError::NoValueForKey(bx)) if *bx == MaybeRelocatable::from(6)
        );
    }
    #[test]
    fn run_dict_read_no_tracker() {
        let hint_code = "dict_tracker = __dict_manager.get_tracker(ids.dict_ptr)\ndict_tracker.current_ptr += ids.DictAccess.SIZE\nids.value = dict_tracker.data[ids.key]";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 3;
        //Create manager
        let mut exec_scopes = scope![("dict_manager", Rc::new(RefCell::new(DictManager::new())))];

        //Insert ids into memory
        vm.segments = segments![((1, 0), 6), ((1, 2), (2, 0))];
        add_segments!(vm, 1);
        let ids_data = ids_data!["key", "value", "dict_ptr"];
        //Execute the hint
        assert_matches!(
            run_hint!(vm, ids_data, hint_code, &mut exec_scopes),
            Err(HintError::NoDictTracker(2))
        );
    }

    #[test]
    fn run_default_dict_new_valid() {
        let hint_code = "if '__dict_manager' not in globals():\n    from starkware.cairo.common.dict import DictManager\n    __dict_manager = DictManager()\n\nmemory[ap] = __dict_manager.new_default_dict(segments, ids.default_value)";
        let mut vm = vm!();
        run_context!(vm, 0, 1, 1);
        //insert ids.default_value into memory
        vm.segments = segments![((1, 0), 17)];
        let ids_data = ids_data!["default_value"];
        let mut exec_scopes = ExecutionScopes::new();
        run_hint!(vm, ids_data, hint_code, &mut exec_scopes).expect("Error while executing hint");
        //third new segment is added for the dictionary
        assert_eq!(vm.segments.memory.data.len(), 3);
        //new segment base (2,0) is inserted into ap (0,0)
        check_memory![vm.segments.memory, ((1, 1), (2, 0))];
        //Check the dict manager has a tracker for segment 2,
        //and that tracker contains the ptr (2,0) and an empty dict
        assert_eq!(
            exec_scopes
                .get_dict_manager()
                .unwrap()
                .borrow()
                .trackers
                .get(&2),
            Some(&DictTracker::new_default_dict(
                relocatable!(2, 0),
                &MaybeRelocatable::from(17),
                None
            ))
        );
    }

    #[test]
    fn run_default_dict_new_no_default_value() {
        let hint_code = "if '__dict_manager' not in globals():\n    from starkware.cairo.common.dict import DictManager\n    __dict_manager = DictManager()\n\nmemory[ap] = __dict_manager.new_default_dict(segments, ids.default_value)";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 1;
        let ids_data = ids_data!["default_value"];
        assert_matches!(
            run_hint!(vm, ids_data, hint_code),
            Err(HintError::UnknownIdentifier(bx)) if bx.as_ref() == "default_value"
        );
    }

    #[test]
    fn run_dict_write_default_valid_empty_dict() {
        let hint_code = "dict_tracker = __dict_manager.get_tracker(ids.dict_ptr)\ndict_tracker.current_ptr += ids.DictAccess.SIZE\nids.dict_ptr.prev_value = dict_tracker.data[ids.key]\ndict_tracker.data[ids.key] = ids.new_value";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 3;
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager_default!(&mut exec_scopes, 2, 2);
        //Insert ids into memory
        vm.segments = segments![((1, 0), 5), ((1, 1), 17), ((1, 2), (2, 0))];
        add_segments!(vm, 1);
        //ids.value (at (1, 0))
        //ids.dict_ptr (2, 0):
        //  dict_ptr.key = (2, 1)
        //  dict_ptr.prev_value = (2, 2)
        //  dict_ptr.new_value = (2, 3)
        let ids_data = ids_data!["key", "new_value", "dict_ptr"];
        //Execute the hint
        assert_matches!(run_hint!(vm, ids_data, hint_code, &mut exec_scopes), Ok(()));
        //Check that the dictionary was updated with the new key-value pair (5, 17)
        check_dictionary![exec_scopes, 2, (5, 17)];
        //Check that the tracker's current_ptr has moved accordingly
        check_dict_ptr!(exec_scopes, 2, (2, 3));
        //Check the value of dict_ptr.prev_value, should be equal to the default_value (2)
        check_memory![vm.segments.memory, ((2, 1), 2)];
    }

    #[test]
    fn run_dict_write_default_valid_overwrite_value() {
        let hint_code = "dict_tracker = __dict_manager.get_tracker(ids.dict_ptr)\ndict_tracker.current_ptr += ids.DictAccess.SIZE\nids.dict_ptr.prev_value = dict_tracker.data[ids.key]\ndict_tracker.data[ids.key] = ids.new_value";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 3;
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager_default!(exec_scopes, 2, 2, (5, 10));
        //Insert ids into memory
        vm.segments = segments![((1, 0), 5), ((1, 1), 17), ((1, 2), (2, 0))];
        add_segments!(vm, 1);
        //ids.value (at (1, 0))
        //ids.dict_ptr (2, 0):
        //  dict_ptr.key = (2, 1)
        //  dict_ptr.prev_value = (2, 2)
        //  dict_ptr.new_value = (2, 3)
        let ids_data = ids_data!["key", "new_value", "dict_ptr"];
        //Execute the hint
        assert_matches!(run_hint!(vm, ids_data, hint_code, &mut exec_scopes), Ok(()));
        //Check that the dictionary was updated with the new key-value pair (5, 17)
        check_dictionary![exec_scopes, 2, (5, 17)];
        //Check that the tracker's current_ptr has moved accordingly
        check_dict_ptr!(exec_scopes, 2, (2, 3));
        //Check the value of dict_ptr.prev_value, should be equal to the previously inserted value (10)
        check_memory![vm.segments.memory, ((2, 1), 10)];
    }

    #[test]
    fn run_dict_write_simple_valid_overwrite_value() {
        let hint_code = "dict_tracker = __dict_manager.get_tracker(ids.dict_ptr)\ndict_tracker.current_ptr += ids.DictAccess.SIZE\nids.dict_ptr.prev_value = dict_tracker.data[ids.key]\ndict_tracker.data[ids.key] = ids.new_value";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 3;
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager!(exec_scopes, 2, (5, 10));
        //Insert ids into memory
        vm.segments = segments![((1, 0), 5), ((1, 1), 17), ((1, 2), (2, 0))];
        add_segments!(vm, 1);
        //ids.value (at (2, 0))
        //ids.dict_ptr (2, 0):
        //  dict_ptr.key = (2, 1)
        //  dict_ptr.prev_value = (2, 2)
        //  dict_ptr.new_value = (2, 3)
        let ids_data = ids_data!["key", "new_value", "dict_ptr"];
        //Execute the hint
        assert_matches!(run_hint!(vm, ids_data, hint_code, &mut exec_scopes), Ok(()));
        //Check that the dictionary was updated with the new key-value pair (5, 17)
        check_dictionary![exec_scopes, 2, (5, 17)];
        //Check that the tracker's current_ptr has moved accordingly
        check_dict_ptr!(exec_scopes, 2, (2, 3));
        check_dict_ptr!(exec_scopes, 2, (2, 3));
        //Check the value of dict_ptr.prev_value, should be equal to the previously inserted value (10)
        check_memory![vm.segments.memory, ((2, 1), 10)];
    }

    #[test]
    fn run_dict_write_simple_valid_cant_write_new_key() {
        let hint_code = "dict_tracker = __dict_manager.get_tracker(ids.dict_ptr)\ndict_tracker.current_ptr += ids.DictAccess.SIZE\nids.dict_ptr.prev_value = dict_tracker.data[ids.key]\ndict_tracker.data[ids.key] = ids.new_value";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 3;
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager!(exec_scopes, 2);
        //Insert ids into memory
        vm.segments = segments![((1, 0), 5), ((1, 1), 17), ((1, 2), (2, 0))];
        add_segments!(vm, 1);
        //ids.value (at (1, 0))
        //ids.dict_ptr (2, 0):
        //  dict_ptr.key = (2, 1)
        //  dict_ptr.prev_value = (2, 2)
        //  dict_ptr.new_value = (2, 3)
        let ids_data = ids_data!["key", "new_value", "dict_ptr"];
        //Execute the hint
        assert_matches!(
            run_hint!(vm, ids_data, hint_code, &mut exec_scopes),
            Err(HintError::NoValueForKey(bx)) if *bx == MaybeRelocatable::from(5)
        );
    }

    #[test]
    fn run_dict_update_simple_valid() {
        let hint_code = "# Verify dict pointer and prev value.\ndict_tracker = __dict_manager.get_tracker(ids.dict_ptr)\ncurrent_value = dict_tracker.data[ids.key]\nassert current_value == ids.prev_value, \\\n    f'Wrong previous value in dict. Got {ids.prev_value}, expected {current_value}.'\n\n# Update value.\ndict_tracker.data[ids.key] = ids.new_value\ndict_tracker.current_ptr += ids.DictAccess.SIZE";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 4;
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager!(exec_scopes, 2, (5, 10));
        //Insert ids into memory
        vm.segments = segments![((1, 0), 5), ((1, 1), 10), ((1, 2), 20), ((1, 3), (2, 0))];
        add_segments!(vm, 1);
        //ids.dict_ptr (2, 0):
        //  dict_ptr.key = (2, 1)
        //  dict_ptr.prev_value = (2, 2)
        //  dict_ptr.new_value = (2, 3)
        let ids_data = ids_data!["key", "prev_value", "new_value", "dict_ptr"];
        //Execute the hint
        assert_matches!(run_hint!(vm, ids_data, hint_code, &mut exec_scopes), Ok(()));
        //Check that the dictionary was updated with the new key-value pair (5, 20)
        check_dictionary![exec_scopes, 2, (5, 20)];
        //Check that the tracker's current_ptr has moved accordingly
        check_dict_ptr!(exec_scopes, 2, (2, 3));
    }

    #[test]
    fn run_dict_update_simple_valid_no_change() {
        let hint_code = "# Verify dict pointer and prev value.\ndict_tracker = __dict_manager.get_tracker(ids.dict_ptr)\ncurrent_value = dict_tracker.data[ids.key]\nassert current_value == ids.prev_value, \\\n    f'Wrong previous value in dict. Got {ids.prev_value}, expected {current_value}.'\n\n# Update value.\ndict_tracker.data[ids.key] = ids.new_value\ndict_tracker.current_ptr += ids.DictAccess.SIZE";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 4;
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager!(exec_scopes, 2, (5, 10));
        //Insert ids into memory
        vm.segments = segments![((1, 0), 5), ((1, 1), 10), ((1, 2), 10), ((1, 3), (2, 0))];
        add_segments!(vm, 1);
        //ids.dict_ptr (2, 0):
        //  dict_ptr.key = (2, 1)
        //  dict_ptr.prev_value = (2, 2)
        //  dict_ptr.new_value = (2, 3)
        let ids_data = ids_data!["key", "prev_value", "new_value", "dict_ptr"];
        //Execute the hint
        assert_matches!(run_hint!(vm, ids_data, hint_code, &mut exec_scopes), Ok(()));
        //Check that the dictionary was updated with the new key-value pair (5, 20)
        check_dictionary![exec_scopes, 2, (5, 10)];
        //Check that the tracker's current_ptr has moved accordingly
        check_dict_ptr!(exec_scopes, 2, (2, 3));
    }

    #[test]
    fn run_dict_update_simple_invalid_wrong_prev_key() {
        let hint_code = "# Verify dict pointer and prev value.\ndict_tracker = __dict_manager.get_tracker(ids.dict_ptr)\ncurrent_value = dict_tracker.data[ids.key]\nassert current_value == ids.prev_value, \\\n    f'Wrong previous value in dict. Got {ids.prev_value}, expected {current_value}.'\n\n# Update value.\ndict_tracker.data[ids.key] = ids.new_value\ndict_tracker.current_ptr += ids.DictAccess.SIZE";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 4;
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager!(exec_scopes, 2, (5, 10));
        //Insert ids into memory
        vm.segments = segments![((1, 0), 5), ((1, 1), 11), ((1, 2), 20), ((1, 3), (2, 0))];
        add_segments!(vm, 1);
        //ids.dict_ptr (2, 0):
        //  dict_ptr.key = (2, 1)
        //  dict_ptr.prev_value = (2, 2)
        //  dict_ptr.new_value = (2, 3)
        let ids_data = ids_data!["key", "prev_value", "new_value", "dict_ptr"];
        //Execute the hint
        assert_matches!(
            run_hint!(vm, ids_data, hint_code, &mut exec_scopes),
            Err(HintError::WrongPrevValue(bx))
            if *bx == (MaybeRelocatable::from(11), MaybeRelocatable::from(10), MaybeRelocatable::from(5))
        );
    }

    #[test]
    fn run_dict_update_simple_invalid_wrong_key() {
        let hint_code = "# Verify dict pointer and prev value.\ndict_tracker = __dict_manager.get_tracker(ids.dict_ptr)\ncurrent_value = dict_tracker.data[ids.key]\nassert current_value == ids.prev_value, \\\n    f'Wrong previous value in dict. Got {ids.prev_value}, expected {current_value}.'\n\n# Update value.\ndict_tracker.data[ids.key] = ids.new_value\ndict_tracker.current_ptr += ids.DictAccess.SIZE";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 4;
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager!(exec_scopes, 2, (5, 10));
        //Insert ids into memory
        vm.segments = segments![((1, 0), 6), ((1, 1), 10), ((1, 2), 10), ((1, 3), (2, 0))];
        add_segments!(vm, 1);
        //ids.dict_ptr (2, 0):
        //  dict_ptr.key = (2, 1)
        //  dict_ptr.prev_value = (2, 2)
        //  dict_ptr.new_value = (2, 3)
        let ids_data = ids_data!["key", "prev_value", "new_value", "dict_ptr"];
        //Execute the hint
        assert_matches!(
            run_hint!(vm, ids_data, hint_code, &mut exec_scopes),
            Err(HintError::NoValueForKey(bx)) if *bx == MaybeRelocatable::from(6)
        );
    }

    #[test]
    fn run_dict_update_default_valid() {
        let hint_code = "# Verify dict pointer and prev value.\ndict_tracker = __dict_manager.get_tracker(ids.dict_ptr)\ncurrent_value = dict_tracker.data[ids.key]\nassert current_value == ids.prev_value, \\\n    f'Wrong previous value in dict. Got {ids.prev_value}, expected {current_value}.'\n\n# Update value.\ndict_tracker.data[ids.key] = ids.new_value\ndict_tracker.current_ptr += ids.DictAccess.SIZE";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 4;
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager!(exec_scopes, 2, (5, 10));
        //Insert ids into memory
        vm.segments = segments![((1, 0), 5), ((1, 1), 10), ((1, 2), 20), ((1, 3), (2, 0))];
        add_segments!(vm, 1);
        //ids.dict_ptr (2, 0):
        //  dict_ptr.key = (2, 1)
        //  dict_ptr.prev_value = (2, 2)
        //  dict_ptr.new_value = (2, 3)
        let ids_data = ids_data!["key", "prev_value", "new_value", "dict_ptr"];
        //Execute the hint
        assert_matches!(run_hint!(vm, ids_data, hint_code, &mut exec_scopes), Ok(()));
        //Check that the dictionary was updated with the new key-value pair (5, 20)
        check_dictionary![exec_scopes, 2, (5, 20)];
        //Check that the tracker's current_ptr has moved accordingly
        check_dict_ptr!(exec_scopes, 2, (2, 3));
    }

    #[test]
    fn run_dict_update_default_valid_no_change() {
        let hint_code = "# Verify dict pointer and prev value.\ndict_tracker = __dict_manager.get_tracker(ids.dict_ptr)\ncurrent_value = dict_tracker.data[ids.key]\nassert current_value == ids.prev_value, \\\n    f'Wrong previous value in dict. Got {ids.prev_value}, expected {current_value}.'\n\n# Update value.\ndict_tracker.data[ids.key] = ids.new_value\ndict_tracker.current_ptr += ids.DictAccess.SIZE";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 4;
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager!(exec_scopes, 2, (5, 10));
        //Insert ids into memory
        vm.segments = segments![((1, 0), 5), ((1, 1), 10), ((1, 2), 10), ((1, 3), (2, 0))];
        add_segments!(vm, 1);
        //ids.dict_ptr (2, 0):
        //  dict_ptr.key = (2, 1)
        //  dict_ptr.prev_value = (2, 2)
        //  dict_ptr.new_value = (2, 3)
        let ids_data = ids_data!["key", "prev_value", "new_value", "dict_ptr"];
        //Execute the hint
        assert_matches!(run_hint!(vm, ids_data, hint_code, &mut exec_scopes), Ok(()));
        //Check that the dictionary was updated with the new key-value pair (5, 10)
        check_dictionary![exec_scopes, 2, (5, 10)];
        //Check that the tracker's current_ptr has moved accordingly
        check_dict_ptr!(exec_scopes, 2, (2, 3));
    }

    #[test]
    fn run_dict_update_default_invalid_wrong_prev_key() {
        let hint_code = "# Verify dict pointer and prev value.\ndict_tracker = __dict_manager.get_tracker(ids.dict_ptr)\ncurrent_value = dict_tracker.data[ids.key]\nassert current_value == ids.prev_value, \\\n    f'Wrong previous value in dict. Got {ids.prev_value}, expected {current_value}.'\n\n# Update value.\ndict_tracker.data[ids.key] = ids.new_value\ndict_tracker.current_ptr += ids.DictAccess.SIZE";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 4;
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager!(exec_scopes, 2, (5, 10));
        //Insert ids into memory
        vm.segments = segments![((1, 0), 5), ((1, 1), 11), ((1, 2), 10), ((1, 3), (2, 0))];
        add_segments!(vm, 1);
        //ids.dict_ptr (2, 0):
        //  dict_ptr.key = (2, 1)
        //  dict_ptr.prev_value = (2, 2)
        //  dict_ptr.new_value = (2, 3)
        let ids_data = ids_data!["key", "prev_value", "new_value", "dict_ptr"];
        //Execute the hint
        assert_matches!(
            run_hint!(vm, ids_data, hint_code, &mut exec_scopes),
            Err(HintError::WrongPrevValue(bx))
            if *bx == (MaybeRelocatable::from(11), MaybeRelocatable::from(10), MaybeRelocatable::from(5))
        );
    }

    #[test]
    fn run_dict_update_default_invalid_wrong_key() {
        let hint_code = "# Verify dict pointer and prev value.\ndict_tracker = __dict_manager.get_tracker(ids.dict_ptr)\ncurrent_value = dict_tracker.data[ids.key]\nassert current_value == ids.prev_value, \\\n    f'Wrong previous value in dict. Got {ids.prev_value}, expected {current_value}.'\n\n# Update value.\ndict_tracker.data[ids.key] = ids.new_value\ndict_tracker.current_ptr += ids.DictAccess.SIZE";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 4;
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager_default!(exec_scopes, 2, 17, (5, 10));
        //Insert ids into memory
        vm.segments = segments![((1, 0), 6), ((1, 1), 10), ((1, 2), 10), ((1, 3), (2, 0))];
        add_segments!(vm, 1);
        //ids.dict_ptr (2, 0):
        //  dict_ptr.key = (2, 1)
        //  dict_ptr.prev_value = (2, 2)
        //  dict_ptr.new_value = (2, 3)
        let ids_data = ids_data!["key", "prev_value", "new_value", "dict_ptr"];
        //Execute the hint
        assert_matches!(
            run_hint!(vm, ids_data, hint_code, &mut exec_scopes),
            Err(HintError::WrongPrevValue(bx))
            if *bx == (MaybeRelocatable::from(10), MaybeRelocatable::from(17), MaybeRelocatable::from(6))
        );
    }

    #[test]
    fn run_dict_update_default_valid_no_key_prev_value_equals_default() {
        let hint_code = "# Verify dict pointer and prev value.\ndict_tracker = __dict_manager.get_tracker(ids.dict_ptr)\ncurrent_value = dict_tracker.data[ids.key]\nassert current_value == ids.prev_value, \\\n    f'Wrong previous value in dict. Got {ids.prev_value}, expected {current_value}.'\n\n# Update value.\ndict_tracker.data[ids.key] = ids.new_value\ndict_tracker.current_ptr += ids.DictAccess.SIZE";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 4;
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager_default!(exec_scopes, 2, 17);
        //Insert ids into memory
        vm.segments = segments![((1, 0), 5), ((1, 1), 17), ((1, 2), 20), ((1, 3), (2, 0))];
        add_segments!(vm, 1);
        //ids.dict_ptr (2, 0):
        //  dict_ptr.key = (2, 1)
        //  dict_ptr.prev_value = (2, 2)
        //  dict_ptr.new_value = (2, 3)
        let ids_data = ids_data!["key", "prev_value", "new_value", "dict_ptr"];
        //Execute the hint
        assert_matches!(run_hint!(vm, ids_data, hint_code, &mut exec_scopes), Ok(()));
        //Check that the dictionary was updated with the new key-value pair (5, 20)
        check_dictionary![exec_scopes, 2, (5, 20)];
        //Check that the tracker's current_ptr has moved accordingly
        check_dict_ptr!(exec_scopes, 2, (2, 3));
    }

    #[test]
    fn run_dict_squash_copy_dict_valid_empty_dict() {
        let hint_code = "# Prepare arguments for dict_new. In particular, the same dictionary values should be copied\n# to the new (squashed) dictionary.\nvm_enter_scope({\n    # Make __dict_manager accessible.\n    '__dict_manager': __dict_manager,\n    # Create a copy of the dict, in case it changes in the future.\n    'initial_dict': dict(__dict_manager.get_dict(ids.dict_accesses_end)),\n})";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 1;
        //ids.dict_access
        vm.segments = segments![((1, 0), (2, 0))];
        add_segments!(vm, 1);
        let ids_data = ids_data!["dict_accesses_end"];
        //Execute the hint
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager!(exec_scopes, 2);
        assert_matches!(run_hint!(vm, ids_data, hint_code, &mut exec_scopes), Ok(()));
        //Check that a new exec scope has been created
        assert_eq!(exec_scopes.data.len(), 2);
        //Check that this scope contains the expected initial-dict
        let variables = exec_scopes.get_local_variables().unwrap();
        assert_eq!(variables.len(), 2); //Two of them, as DictManager is also there
        assert_eq!(
            variables
                .get("initial_dict")
                .unwrap()
                .downcast_ref::<HashMap<MaybeRelocatable, MaybeRelocatable>>(),
            Some(&HashMap::<MaybeRelocatable, MaybeRelocatable>::new())
        );
    }

    #[test]
    fn run_dict_squash_copy_dict_valid_non_empty_dict() {
        let hint_code = "# Prepare arguments for dict_new. In particular, the same dictionary values should be copied\n# to the new (squashed) dictionary.\nvm_enter_scope({\n    # Make __dict_manager accessible.\n    '__dict_manager': __dict_manager,\n    # Create a copy of the dict, in case it changes in the future.\n    'initial_dict': dict(__dict_manager.get_dict(ids.dict_accesses_end)),\n})";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 1;
        vm.segments = segments![((1, 0), (2, 0))];
        add_segments!(vm, 1);
        let ids_data = ids_data!["dict_accesses_end"];
        //Execute the hint
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager!(exec_scopes, 2, (1, 2), (3, 4), (5, 6));
        assert_matches!(run_hint!(vm, ids_data, hint_code, &mut exec_scopes), Ok(()));
        //Check that a new exec scope has been created
        assert_eq!(exec_scopes.data.len(), 2);
        //Check that this scope contains the expected initial-dict
        let variables = exec_scopes.get_local_variables().unwrap();
        assert_eq!(variables.len(), 2); //Two of them, as DictManager is also there
        assert_eq!(
            variables
                .get("initial_dict")
                .unwrap()
                .downcast_ref::<HashMap<MaybeRelocatable, MaybeRelocatable>>(),
            Some(&HashMap::from([
                (MaybeRelocatable::from(1), MaybeRelocatable::from(2)),
                (MaybeRelocatable::from(3), MaybeRelocatable::from(4)),
                (MaybeRelocatable::from(5), MaybeRelocatable::from(6))
            ]))
        );
    }

    #[test]
    fn run_dict_squash_copy_dict_invalid_no_dict() {
        let hint_code = "# Prepare arguments for dict_new. In particular, the same dictionary values should be copied\n# to the new (squashed) dictionary.\nvm_enter_scope({\n    # Make __dict_manager accessible.\n    '__dict_manager': __dict_manager,\n    # Create a copy of the dict, in case it changes in the future.\n    'initial_dict': dict(__dict_manager.get_dict(ids.dict_accesses_end)),\n})";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 1;
        //Create manager
        let dict_manager = DictManager::new();
        let mut exec_scopes = scope![("dict_manager", Rc::new(RefCell::new(dict_manager)))];

        vm.segments = segments![((1, 0), (2, 0))];
        add_segments!(vm, 1);
        let ids_data = ids_data!["dict_accesses_end"];
        //Execute the hint
        assert_matches!(
            run_hint!(vm, ids_data, hint_code, &mut exec_scopes),
            Err(HintError::NoDictTracker(2))
        );
    }

    #[test]
    fn run_dict_squash_update_ptr_no_tracker() {
        let hint_code = "# Update the DictTracker's current_ptr to point to the end of the squashed dict.\n__dict_manager.get_tracker(ids.squashed_dict_start).current_ptr = \\\n    ids.squashed_dict_end.address_";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 2;
        //Create manager
        let dict_manager = DictManager::new();
        let mut exec_scopes = scope![("dict_manager", Rc::new(RefCell::new(dict_manager)))];
        vm.segments = segments![((1, 0), (2, 0)), ((1, 1), (2, 3))];
        add_segments!(vm, 1);
        //Create ids
        let ids_data = ids_data!["squashed_dict_start", "squashed_dict_end"];
        //Execute the hint
        assert_matches!(
            run_hint!(vm, ids_data, hint_code, &mut exec_scopes),
            Err(HintError::NoDictTracker(2))
        );
    }

    #[test]
    fn run_dict_squash_update_ptr_valid() {
        let hint_code = "# Update the DictTracker's current_ptr to point to the end of the squashed dict.\n__dict_manager.get_tracker(ids.squashed_dict_start).current_ptr = \\\n    ids.squashed_dict_end.address_";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 2;
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager![exec_scopes, 2, (1, 2)];
        //ids.squash_dict_start
        vm.segments = segments![((1, 0), (2, 0)), ((1, 1), (2, 3))];
        add_segments!(vm, 1);
        //Create ids
        let ids_data = ids_data!["squashed_dict_start", "squashed_dict_end"];
        //Execute the hint
        assert_matches!(run_hint!(vm, ids_data, hint_code, &mut exec_scopes), Ok(()));
        //Check the updated pointer
        check_dict_ptr!(exec_scopes, 2, (2, 3));
    }

    #[test]
    fn run_dict_squash_update_ptr_mismatched_dict_ptr() {
        let hint_code = "# Update the DictTracker's current_ptr to point to the end of the squashed dict.\n__dict_manager.get_tracker(ids.squashed_dict_start).current_ptr = \\\n    ids.squashed_dict_end.address_";
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 2;
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager!(exec_scopes, 2, (1, 2));
        vm.segments = segments![((1, 0), (2, 3)), ((1, 1), (2, 6))];
        add_segments!(vm, 1);
        //Create ids
        let ids_data = ids_data!["squashed_dict_start", "squashed_dict_end"];
        //Execute the hint
        assert_matches!(
            run_hint!(vm, ids_data, hint_code, &mut exec_scopes),
            Err(HintError::MismatchedDictPtr(bx)) if *bx == (relocatable!(2, 0), relocatable!(2, 3))
        );
    }

    #[test]
    fn run_dict_write_valid_relocatable_new_value() {
        let mut vm = vm!();
        //Initialize fp
        vm.run_context.fp = 3;
        let mut exec_scopes = ExecutionScopes::new();
        dict_manager_default!(&mut exec_scopes, 2, 2);
        // First we run dict_write hint
        //Insert ids into memory
        vm.segments = segments![((1, 0), 5), ((1, 1), (1, 7)), ((1, 2), (2, 0))];
        add_segments!(vm, 1);
        // new_value here is (1,7)
        let ids_data = ids_data!["key", "new_value", "dict_ptr"];
        //Execute the hint
        assert_matches!(
            run_hint!(vm, ids_data, hint_code::DICT_WRITE, &mut exec_scopes),
            Ok(())
        );
        // Check that our relocatable was written into the dict
        let expected_dict = Dictionary::DefaultDictionary {
            dict: HashMap::from([(MaybeRelocatable::from(5), MaybeRelocatable::from((1, 7)))]),
            default_value: MaybeRelocatable::from(2),
        };
        let expeced_dict_tracker = DictTracker {
            data: expected_dict,
            current_ptr: Relocatable::from((2, 3)),
        };
        assert_eq!(
            exec_scopes
                .get_dict_manager()
                .unwrap()
                .borrow()
                .trackers
                .get(&2),
            Some(&expeced_dict_tracker)
        );
    }
}
