//! The libkernel hardware queries and the POSIX priority bounds as a console answered them from a
//! title's process (obSCEne `REQ-20261012T1827Z-1a71`, `000-boot/hardware-queries`,
//! `report-1791833977.txt` lines 696-723).

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

fn implementation(name: &str) -> GuestFn {
    orbistoun_kernel::implementations()
        .iter()
        .find(|(n, _)| *n == name)
        .map_or_else(|| panic!("{name} is not implemented"), |(_, f)| *f)
}

fn call(name: &str, args: &[u64]) -> u64 {
    let mut regs = [0_u64; GUEST_ARG_REGISTERS];
    regs[..args.len()].copy_from_slice(args);
    implementation(name)(&regs)
}

#[test]
fn the_hardware_queries_answer_as_measured() {
    assert_eq!(call("sceKernelGetCpuFrequency", &[]), 0);
    let out = [0xcc_u8; 32];
    assert_eq!(
        call("sceKernelGetCpuTemperature", &[out.as_ptr() as u64]),
        0x8002_0002
    );
    assert_eq!(call("sceKernelGetCpuTemperature", &[0]), 0x8002_0002);
    for sensor in [0, 1, 0xff] {
        assert_eq!(
            call(
                "sceKernelGetSocSensorTemperature",
                &[sensor, out.as_ptr() as u64]
            ),
            0x8002_0002,
            "sensor {sensor}"
        );
    }
    assert_eq!(out, [0xcc; 32], "nothing written");
}

#[test]
fn the_priority_bounds_are_the_measured_ones() {
    for (policy, min, max) in [
        (0, u64::MAX, u64::MAX),
        (1, 0x100, 0x2ff),
        (2, u64::MAX, u64::MAX),
        (3, 0x100, 0x2ff),
        (99, u64::MAX, u64::MAX),
    ] {
        assert_eq!(call("sched_get_priority_max", &[policy]), max, "{policy}");
        assert_eq!(call("sched_get_priority_min", &[policy]), min, "{policy}");
    }
}
