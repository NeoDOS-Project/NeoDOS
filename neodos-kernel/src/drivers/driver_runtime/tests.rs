//! Driver runtime tests.

use super::*;

// ── Test suite: Driver State Machine + Certification Pipeline ──

pub fn register_driver_state_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_ne;
    use crate::test_true;

    // ── Transition matrix tests ──

    test_case!("dstate_valid_loaded_to_init", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        test_eq!(rt.get(id).unwrap().state, DriverState::Loaded);
        test_true!(rt.try_transition(id, DriverState::Initialized).is_ok());
        test_eq!(rt.get(id).unwrap().state, DriverState::Initialized);
        rt.remove(id);
    });

    test_case!("dstate_valid_init_to_registered", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        rt.try_transition(id, DriverState::Initialized).ok();
        test_true!(rt.try_transition(id, DriverState::Registered).is_ok());
        test_eq!(rt.get(id).unwrap().state, DriverState::Registered);
        rt.remove(id);
    });

    test_case!("dstate_valid_registered_to_bound", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        rt.try_transition(id, DriverState::Initialized).ok();
        rt.try_transition(id, DriverState::Registered).ok();
        test_true!(rt.try_transition(id, DriverState::Bound).is_ok());
        test_eq!(rt.get(id).unwrap().state, DriverState::Bound);
        rt.remove(id);
    });

    test_case!("dstate_valid_bound_to_active", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        rt.try_transition(id, DriverState::Initialized).ok();
        rt.try_transition(id, DriverState::Registered).ok();
        rt.try_transition(id, DriverState::Bound).ok();
        test_true!(rt.try_transition(id, DriverState::Active).is_ok());
        test_eq!(rt.get(id).unwrap().state, DriverState::Active);
        rt.remove(id);
    });

    // ── Invalid transition tests ──

    test_case!("dstate_invalid_skip_init", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        // Cannot skip Initialized — going directly to Registered should fail
        test_true!(rt.try_transition(id, DriverState::Registered).is_err());
        test_eq!(rt.get(id).unwrap().state, DriverState::Loaded);
        rt.remove(id);
    });

    test_case!("dstate_invalid_skip_registered", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        rt.try_transition(id, DriverState::Initialized).ok();
        // Cannot skip Registered — going directly to Bound should fail
        test_true!(rt.try_transition(id, DriverState::Bound).is_err());
        test_eq!(rt.get(id).unwrap().state, DriverState::Initialized);
        rt.remove(id);
    });

    test_case!("dstate_invalid_skip_bound", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        rt.try_transition(id, DriverState::Initialized).ok();
        rt.try_transition(id, DriverState::Registered).ok();
        // Cannot skip Bound — going directly to Active should fail
        test_true!(rt.try_transition(id, DriverState::Active).is_err());
        test_eq!(rt.get(id).unwrap().state, DriverState::Registered);
        rt.remove(id);
    });

    test_case!("dstate_invalid_skip_all", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        // Loaded → Active is impossible (6 steps missing)
        test_true!(rt.try_transition(id, DriverState::Active).is_err());
        test_eq!(rt.get(id).unwrap().state, DriverState::Loaded);
        rt.remove(id);
    });

    // ── Fault / Unload transition tests ──

    test_case!("dstate_any_to_faulted", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        // Any state can go to Faulted
        test_true!(rt.try_transition(id, DriverState::Faulted).is_ok());
        test_eq!(rt.get(id).unwrap().state, DriverState::Faulted);
        rt.remove(id);
    });

    test_case!("dstate_any_to_unloaded", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        test_true!(rt.try_transition(id, DriverState::Unloaded).is_ok());
        test_eq!(rt.get(id).unwrap().state, DriverState::Unloaded);
        rt.remove(id);
    });

    test_case!("dstate_faulted_to_active_fails", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        rt.try_transition(id, DriverState::Faulted).ok();
        // Cannot recover from Faulted to Active (must go through Unloaded)
        test_true!(rt.try_transition(id, DriverState::Active).is_err());
        rt.remove(id);
    });

    // ── Certification pipeline tests ──

    test_case!("dstate_certify_full_pipeline", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        // Walk through all 5 stages
        rt.try_transition(id, DriverState::Initialized).ok();
        rt.try_transition(id, DriverState::Registered).ok();
        rt.try_transition(id, DriverState::Bound).ok();
        // Now certify — should succeed since all prior stages completed
        test_true!(rt.certify_and_activate(id).is_ok());
        test_eq!(rt.get(id).unwrap().state, DriverState::Active);
        test_eq!(rt.get(id).unwrap().last_error, 0);
        test_eq!(rt.get(id).unwrap().certification_step, 0);
        rt.remove(id);
    });

    test_case!("dstate_certify_incomplete_pipeline", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        // Only go through Loaded → Initialized → Registered (skip Bound)
        rt.try_transition(id, DriverState::Initialized).ok();
        rt.try_transition(id, DriverState::Registered).ok();
        // Should NOT be able to certify — pipeline incomplete
        test_true!(rt.certify_and_activate(id).is_err());
        test_eq!(rt.get(id).unwrap().state, DriverState::Registered);
        test_ne!(rt.get(id).unwrap().last_error, 0);
        rt.remove(id);
    });

    test_case!("dstate_certify_not_initialized", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        // Only Loaded — can't certify
        test_true!(rt.certify_and_activate(id).is_err());
        test_eq!(rt.get(id).unwrap().state, DriverState::Loaded);
        rt.remove(id);
    });

    test_case!("dstate_certify_not_bound", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        rt.try_transition(id, DriverState::Initialized).ok();
        rt.try_transition(id, DriverState::Registered).ok();
        // Not Bound — can't certify
        test_true!(rt.certify_and_activate(id).is_err());
        test_eq!(rt.get(id).unwrap().certification_step, PipelineStep::Certification as u8);
        rt.remove(id);
    });

    test_case!("dstate_set_error_and_fault", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        rt.set_error(id, ERR_OUT_OF_MEMORY, true);
        let drv = rt.get(id).unwrap();
        test_eq!(drv.state, DriverState::Faulted);
        test_eq!(drv.last_error, ERR_OUT_OF_MEMORY);
        rt.remove(id);
    });

    test_case!("dstate_set_error_no_fault", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        rt.set_error(id, ERR_CERTIFICATION_FAILED, false);
        let drv = rt.get(id).unwrap();
        // Should still be Loaded (not faulted)
        test_eq!(drv.state, DriverState::Loaded);
        test_eq!(drv.last_error, ERR_CERTIFICATION_FAILED);
        rt.remove(id);
    });

    // ── Active count tests ──

    test_case!("dstate_active_count", {
        let mut rt = DriverRuntime::new();
        let id1 = rt.register("drv1", NemDriverType::Null, 1, 0).unwrap();
        let _id2 = rt.register("drv2", NemDriverType::Echo, 1, 0).unwrap();
        test_eq!(rt.active_count(), 0); // none active yet
        // Fully certify drv1
        rt.try_transition(id1, DriverState::Initialized).ok();
        rt.try_transition(id1, DriverState::Registered).ok();
        rt.try_transition(id1, DriverState::Bound).ok();
        rt.certify_and_activate(id1).ok();
        test_eq!(rt.active_count(), 1);
        // drv2 should not affect active_count
        test_eq!(rt.active_count(), 1);
        rt.remove(id1); rt.remove(_id2);
    });

    test_case!("dstate_loaded_count", {
        let mut rt = DriverRuntime::new();
        let id1 = rt.register("drv1", NemDriverType::Null, 1, 0).unwrap();
        let _id2 = rt.register("drv2", NemDriverType::Echo, 1, 0).unwrap();
        test_eq!(rt.loaded_count(), 2); // both loaded, none active
        rt.try_transition(id1, DriverState::Initialized).ok();
        rt.try_transition(id1, DriverState::Registered).ok();
        rt.try_transition(id1, DriverState::Bound).ok();
        rt.certify_and_activate(id1).ok();
        test_eq!(rt.loaded_count(), 1); // drv2 still loaded-not-active
        rt.remove(id1); rt.remove(_id2);
    });

    // ── Inactive reason tests ──

    test_case!("dstate_inactive_reason", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        let drv = rt.get(id).unwrap();
        test_ne!(drv.inactive_reason(), "Driver IS active");
        test_true!(drv.inactive_reason().contains("Loaded"));
        // Advanced to Init
        rt.try_transition(id, DriverState::Initialized).ok();
        let drv = rt.get(id).unwrap();
        test_true!(drv.inactive_reason().contains("Initialized"));
        rt.remove(id);
    });

    // ── Pipeline progress test ──

    test_case!("dstate_pipeline_progress", {
        let mut rt = DriverRuntime::new();
        let id = rt.register("test", NemDriverType::Null, 1, 0).unwrap();
        let prog = rt.get(id).unwrap().pipeline_progress();
        test_eq!(prog, [false, false, false, false, false]);
        rt.try_transition(id, DriverState::Initialized).ok();
        let prog = rt.get(id).unwrap().pipeline_progress();
        test_eq!(prog, [true, false, false, false, false]);
        rt.try_transition(id, DriverState::Registered).ok();
        let prog = rt.get(id).unwrap().pipeline_progress();
        test_eq!(prog, [true, true, false, false, false]);
        rt.try_transition(id, DriverState::Bound).ok();
        let prog = rt.get(id).unwrap().pipeline_progress();
        test_eq!(prog, [true, true, true, false, false]);
        rt.try_transition(id, DriverState::Active).ok();
        let prog = rt.get(id).unwrap().pipeline_progress();
        test_eq!(prog, [true, true, true, true, true]);
        rt.remove(id);
    });
}

pub fn register_driver_certification_tests() {
    register_driver_state_tests();
}
