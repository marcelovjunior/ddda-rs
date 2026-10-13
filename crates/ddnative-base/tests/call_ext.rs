//! L170: `call_ext`'s handler slot (a pointer read, no reference count):
//! nested calls find the handler, a handler replaced while it runs keeps
//! running and the nested calls after it get the new one, a panic leaves
//! the handler installed, and a thread without one panics.
use ddnative_base::mem::Sparse;
use ddnative_base::x86::{call_ext, set_ext_calls, Cpu};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn nested_calls_find_the_handler() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let s = seen.clone();
    set_ext_calls(Some(Box::new(move |m, cpu, target| {
        s.borrow_mut().push(target);
        if target > 0 {
            call_ext(m, cpu, target - 1);
        }
    })));
    call_ext(&mut Sparse::default(), &mut Cpu::default(), 3);
    assert_eq!(*seen.borrow(), [3, 2, 1, 0]);
    set_ext_calls(None);
}

#[test]
fn a_handler_replaced_while_it_runs_keeps_running() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (s, t) = (seen.clone(), seen.clone());
    // the old handler owns data it reads after replacing itself
    let owned = vec![7u32; 64];
    set_ext_calls(Some(Box::new(move |m, cpu, target| {
        let s2 = t.clone();
        let old = set_ext_calls(Some(Box::new(move |_, _, target| s2.borrow_mut().push(target + 100))));
        drop(old);
        call_ext(m, cpu, target);
        s.borrow_mut().push(owned.iter().sum::<u32>() + target);
    })));
    call_ext(&mut Sparse::default(), &mut Cpu::default(), 1);
    assert_eq!(*seen.borrow(), [101, 7 * 64 + 1]);
    // the new one is installed
    call_ext(&mut Sparse::default(), &mut Cpu::default(), 5);
    assert_eq!(seen.borrow().last(), Some(&105));
    set_ext_calls(None);
}

#[test]
fn a_panicking_handler_stays_installed() {
    let calls = Rc::new(RefCell::new(0));
    let c = calls.clone();
    set_ext_calls(Some(Box::new(move |_, _, target| {
        *c.borrow_mut() += 1;
        assert!(target != 9, "nine");
    })));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| call_ext(&mut Sparse::default(), &mut Cpu::default(), 9)));
    assert!(r.is_err());
    call_ext(&mut Sparse::default(), &mut Cpu::default(), 1);
    assert_eq!(*calls.borrow(), 2);
    set_ext_calls(None);
}

#[test]
fn no_handler_panics() {
    let r = std::panic::catch_unwind(|| call_ext(&mut Sparse::default(), &mut Cpu::default(), 0x1234));
    let e = r.unwrap_err();
    assert!(e.downcast_ref::<String>().unwrap().contains("0x00001234"));
}
