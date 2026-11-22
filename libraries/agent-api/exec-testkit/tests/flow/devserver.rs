//! The port a test gives a dev server that a headless Chromium then loads: never one the browser refuses.
use exec_testkit::devserver::{browser_safe, draw, free_port};

const CHROMIUM_UNSAFE: &[u16] = &[
    1, 7, 9, 11, 13, 15, 17, 19, 20, 21, 22, 23, 25, 37, 42, 43, 53, 69, 77, 79, 87, 95, 101, 102, 103, 104, 109, 110, 111, 113, 115, 117, 119, 123, 135, 137, 139, 143, 161, 179, 389, 427,
    465, 512, 513, 514, 515, 526, 530, 531, 532, 540, 548, 554, 556, 563, 587, 601, 636, 989, 990, 993, 995, 1719, 1720, 1723, 2049, 3659, 4045, 4190, 5060, 5061, 6000, 6566, 6665, 6666,
    6667, 6668, 6669, 6679, 6697, 10080,
];

#[test]
fn the_ports_a_browser_refuses_are_exactly_the_ones_chromium_lists() {
    // Given Chromium's list of unsafe ports
    let listed = CHROMIUM_UNSAFE.to_vec();
    // When every port is asked
    let refused: Vec<u16> = (0..=u16::MAX).filter(|p| !browser_safe(*p)).collect();
    // Then the refused ones are the listed ones and no others
    assert_eq!(refused, { let mut sorted = listed; sorted.sort_unstable(); sorted });
}

#[test]
fn no_draw_over_the_whole_range_the_allocator_takes_from_is_an_unsafe_port_and_every_safe_port_of_the_range_can_be_drawn() {
    // Given every step of the allocator (one stride of it reaches the whole range) for a spread of process ids, the largest ones too
    let mut drawn = std::collections::BTreeSet::new();
    // When each pair is drawn
    for pid in (0..40).chain([4_194_304, u32::MAX]) {
        for step in 0..7000 {
            if let Some(port) = draw(pid, step) {
                // Then it is in the preview range and a browser loads it
                assert!((3000..=9999).contains(&port) && browser_safe(port), "{pid} {step} drew {port}");
                drawn.insert(port);
            }
        }
    }
    // And every safe port of the range is reachable, so only the unsafe ones were given up
    let safe: std::collections::BTreeSet<u16> = (3000..=9999).filter(|p| browser_safe(*p)).collect();
    assert_eq!(drawn, safe);
    assert!([3659, 4045, 4190, 5060, 5061, 6000, 6566, 6665, 6666, 6667, 6668, 6669, 6679, 6697].iter().all(|p| !drawn.contains(p)));
}

#[test]
fn many_free_ports_taken_in_a_row_are_all_ports_a_browser_loads_and_none_repeats() {
    // Given a process that takes ports one after the other
    // When it takes a few hundred, each held until the process ends
    let ports: Vec<u16> = (0..300).map(|_| free_port()).collect();
    // Then none is refused by Chromium and none was handed out twice
    assert!(ports.iter().all(|p| browser_safe(*p)), "{ports:?}");
    assert_eq!(ports.iter().collect::<std::collections::BTreeSet<_>>().len(), ports.len());
}
