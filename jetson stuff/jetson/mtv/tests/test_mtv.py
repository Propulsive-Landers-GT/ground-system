import importlib
import io
import csv
import sys
import tempfile
import unittest
from collections import deque
from contextlib import ExitStack, redirect_stderr, redirect_stdout
from itertools import product
from pathlib import Path
from types import ModuleType
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
gpio = ModuleType("Jetson.GPIO")
for name in ("BOARD", "OUT", "IN", "BOTH", "LOW", "HIGH"):
    setattr(gpio, name, name)
for name in ("setmode", "setup", "cleanup", "PWM", "input", "add_event_detect", "remove_event_detect"):
    setattr(gpio, name, Mock())
jetson = ModuleType("Jetson")
jetson.GPIO = gpio
with patch.dict(sys.modules, {"Jetson": jetson, "Jetson.GPIO": gpio}):
    import mtv_module
    import mtv_servo
    import procedure
    import procedure_io
    import rcs_procedure
    import rcs_load_test
    import calibrate_mtv
    import mtv_calibration
    launchers = [importlib.import_module(name) for name in ("hotfire", "coldflow", "kaboom_test")]

CALIBRATION = """# Calibration fixture
# BOARD pins: servo1=15, servo2=32
# Logical angles: lead reference=40, offset=5, valve=30;
DEFAULT_SERVO1_PIN = 15
DEFAULT_SERVO2_PIN = 32
CALIBRATION_VALVE_ANGLE = 30
DEFAULT_CLOSE_ANGLE = 20
DEFAULT_SERVO2_OFFSET = 5
DEFAULT_SERVO1_DIRECTION = -1
DEFAULT_SERVO2_DIRECTION = -1
DEFAULT_VALVE_DIRECTION = -1
DEFAULT_GEAR_RATIO = 2
DEFAULT_SERVO_RANGE = 355
"""


class Clock:
    def __init__(self):
        self.now = 1000.0

    def sleep(self, seconds):
        self.now += seconds


class Port:
    def __init__(self, *args, **kwargs):
        self.lines = deque([b"connected\n"])
        self.writes = []
        self.closed = False
        self.stream = None
        self.before_read = lambda: None

    @property
    def in_waiting(self):
        return bool(self.lines or self.stream)

    def readline(self):
        self.before_read()
        return self.lines.popleft() if self.lines else self.stream or b""

    def write(self, data):
        self.writes.append(data.decode())
        if data.endswith(b" setup"):
            self.lines.append(data.split()[0] + b" loadcell ready\n")
        return len(data)

    def flush(self):
        pass

    def setDTR(self, value):
        pass

    def close(self):
        self.closed = True


class HardwareTests(unittest.TestCase):
    def setUp(self):
        self.stack = ExitStack()
        self.addCleanup(self.stack.close)
        modules = (mtv_module, mtv_servo, procedure, procedure_io, rcs_procedure, rcs_load_test,
                   calibrate_mtv, *launchers)
        self.stack.enter_context(patch.dict(sys.modules, {
            "Jetson": jetson, "Jetson.GPIO": gpio, **{module.__name__: module for module in modules}
        }))
        self.stack.enter_context(redirect_stdout(io.StringIO()))
        self.stack.enter_context(redirect_stderr(io.StringIO()))
        self.output_path = Path(self.stack.enter_context(tempfile.TemporaryDirectory())) / "calibration.txt"
        self.stack.enter_context(patch.object(calibrate_mtv, "DEFAULT_OUTPUT", self.output_path))
        self.clock = Clock()
        self.stack.enter_context(patch.object(procedure_io.time, "monotonic", side_effect=lambda: self.clock.now))
        self.stack.enter_context(patch.object(procedure_io.time, "sleep", side_effect=self.clock.sleep))
        self.pwms = []
        for name in ("setmode", "setup", "cleanup", "PWM", "input", "add_event_detect", "remove_event_detect"):
            getattr(gpio, name).reset_mock(return_value=True, side_effect=True)

        def make_pwm(*args):
            pwm = Mock()
            self.pwms.append(pwm)
            return pwm

        gpio.PWM.side_effect = make_pwm

    def make_runner(self, module, commands=None):
        ports = [Port(), Port()]
        self.stack.enter_context(patch.object(module.serial, "Serial", side_effect=ports))
        self.stack.enter_context(patch.object(module, "manual_control", side_effect=[("start", "", ""), ("end", "", "")]))
        self.stack.enter_context(patch.object(module, "terminal_input", return_value=None))
        log = io.StringIO()
        self.stack.enter_context(patch.object(module, "open", return_value=log, create=True))
        pfs = mtv_module.PFS_Profile(commands if commands is not None else [("omv close", 0), ("igv close", 0.1)])
        if module is procedure:
            runner = module.Procedure(mtv_module.Angle_Profile([("hold", 20, 0, 0.1)]), pfs)
        else:
            runner = module.RCS_Procedure(pfs)
        return runner, ports, log

    def assert_closed(self, runner, ports, log=None):
        self.assertTrue(runner._closed)
        self.assertTrue(all(port.closed for port in ports))
        self.assertIn("reset default", ports[0].writes)
        gpio.cleanup.assert_called()
        for pwm in self.pwms:
            pwm.stop.assert_called()
        if log is not None:
            self.assertTrue(log.closed)

    def test_scheduler_drains_overdue_commands_in_order_including_duplicates(self):
        expected = ["first", "second", "second", "last"]
        profile = mtv_module.PFS_Profile(list(zip(expected, [0, 1, 1, 2])))
        profile.start()
        self.assertEqual(profile.update(), "first")
        self.clock.now += 10
        actual = ["first"]
        while not profile.done():
            actual.append(profile.update())
        self.assertEqual(actual, expected)
        self.assertIsNone(profile.update())

    def test_scheduler_exact_deadline_and_reuse(self):
        profile = mtv_module.PFS_Profile([("first", 0), ("last", 1)])
        for _ in range(2):
            profile.start()
            self.assertEqual(profile.update(), "first")
            self.clock.now += 1
            self.assertFalse(profile.done())
            self.assertEqual(profile.update(), "last")
            self.assertTrue(profile.done())
        self.assertTrue(mtv_module.PFS_Profile([]).done())

    def test_angle_profile_stays_closed_before_and_after_delayed_ramp(self):
        profile = mtv_module.Angle_Profile([("ramp", 10, 30, 5, 10)])
        self.assertEqual(profile.start(), 0)
        self.clock.now += 5
        self.assertEqual(profile.update(), 10)
        self.clock.now += 2.5
        self.assertEqual(profile.update(), 20)
        self.clock.now += 2.5
        self.assertTrue(profile.done())
        self.assertEqual(profile.update(), 0)

    def test_invalid_profiles_are_rejected(self):
        for commands in ([], [("ramp", 0, 90, 1, 1)], [("hold", 100, 0, 1)],
                         [("hold", 0, 0, 2), ("hold", 0, 1, 3)], [("hold", 0, 0, float("nan"))]):
            with self.subTest(commands=commands), self.assertRaises(ValueError):
                mtv_module.Angle_Profile(commands)
        with self.assertRaises(ValueError):
            mtv_module.PFS_Profile([("first", 2), ("second", 1)])

    def test_servos_initialize_closed_and_both_receive_shutdown_attempts(self):
        mtv = mtv_module.MTV(15, 33)
        for pwm in self.pwms:
            pwm.start.assert_called_once_with(mtv.servo1._get_pwm(88))
        self.pwms[0].ChangeDutyCycle.side_effect = OSError("servo one failed")
        self.pwms[0].stop.side_effect = OSError("stop one failed")
        procedure_io.shutdown(mtv)
        self.pwms[1].ChangeDutyCycle.assert_called_with(mtv.servo2._get_pwm(88))
        self.pwms[1].stop.assert_called_once()
        gpio.cleanup.assert_called_once()

    def test_second_servo_failure_cleans_up_first(self):
        pwm = Mock()
        gpio.PWM.side_effect = [pwm, OSError("second PWM failed")]
        with self.assertRaises(OSError):
            mtv_module.MTV(15, 33)
        pwm.stop.assert_called_once()
        gpio.cleanup.assert_called()

    def test_pwm_start_failure_stops_and_releases_channel(self):
        pwm = Mock()
        pwm.start.side_effect = KeyboardInterrupt
        gpio.PWM.side_effect = None
        gpio.PWM.return_value = pwm
        with self.assertRaises(KeyboardInterrupt):
            mtv_servo.Servo(15)
        pwm.stop.assert_called_once()
        gpio.cleanup.assert_called_once_with(15)

    def test_cleanup_continues_after_independent_failures(self):
        mtv, actuation, loadcell, log = Mock(), Mock(), Mock(), Mock()
        mtv.command.side_effect = OSError("command failed")
        mtv.stop.side_effect = OSError("stop failed")
        actuation.write.side_effect = OSError("USB disconnected")
        actuation.close.side_effect = OSError("close failed")
        log.close.side_effect = OSError("disk failed")
        procedure_io.shutdown(mtv, actuation, loadcell, log)
        self.assertEqual(actuation.write.call_count, 3)
        mtv.stop.assert_called_once()
        loadcell.close.assert_called_once()
        gpio.cleanup.assert_called_once()

    def test_second_serial_open_failure_closes_every_acquired_resource(self):
        for module in (procedure, rcs_procedure):
            with self.subTest(module=module.__name__):
                port = Port()
                with patch.object(module.serial, "Serial", side_effect=[port, OSError("loadcell missing")]):
                    pfs = mtv_module.PFS_Profile([])
                    with self.assertRaises(OSError):
                        if module is procedure:
                            module.Procedure(mtv_module.Angle_Profile([("hold", 0, 0, 1)]), pfs)
                        else:
                            module.RCS_Procedure(pfs)
                self.assertTrue(port.closed)
                self.assertIn("reset default", port.writes)
                for pwm in self.pwms:
                    pwm.stop.assert_called()
                gpio.cleanup.assert_called()

    def test_cleanup_still_runs_when_error_reporting_fails(self):
        failed_action = Mock(side_effect=OSError("device failed"))
        stderr = Mock()
        stderr.write.side_effect = OSError("terminal disconnected")
        with patch.object(sys, "stderr", stderr):
            procedure_io.cleanup(failed_action, gpio.cleanup)
        gpio.cleanup.assert_called_once()

    def test_both_runners_drain_commands_despite_continuous_telemetry_and_stall(self):
        expected = ["omv close", "igv close", "igv close", "ofill close"]
        for module in (procedure, rcs_procedure):
            with self.subTest(module=module.__name__):
                runner, ports, log = self.make_runner(module, list(zip(expected, [0, 0.1, 0.1, 0.2])))
                ports[1].stream = b"engine nitrous:bad engine:1 rcs:2\n"
                ports[1].before_read = lambda: self.clock.sleep(2)
                runner.run()
                for port in ports:
                    self.assertEqual([cmd for cmd in port.writes if cmd in expected], expected)
                if module is procedure:
                    self.assertEqual(runner.pfs_profile.start_time, runner.angle_profile.start_time)
                self.assert_closed(runner, ports, log)

    def test_interrupt_and_runtime_error_during_execution_cleanup_both_runners(self):
        for module in (procedure, rcs_procedure):
            for failure in (KeyboardInterrupt, OSError):
                with self.subTest(module=module.__name__, failure=failure):
                    runner, ports, log = self.make_runner(module)
                    with patch.object(module, "terminal_input", side_effect=failure):
                        with self.assertRaises(failure):
                            runner.run()
                    self.assert_closed(runner, ports, log)

    def test_abort_closes_without_recursive_restart(self):
        for module in (procedure, rcs_procedure):
            with self.subTest(module=module.__name__):
                runner, ports, log = self.make_runner(module)
                with patch.object(module, "terminal_input", return_value="exit"):
                    runner.run()
                self.assertEqual(module.manual_control.call_count, 1)
                self.assertNotIn("omv close", ports[0].writes)
                self.assert_closed(runner, ports, log)

    def test_handshake_timeout_and_log_open_failure_cleanup(self):
        for module in (procedure, rcs_procedure):
            for phase in ("handshake", "log"):
                with self.subTest(module=module.__name__, phase=phase):
                    runner, ports, _ = self.make_runner(module)
                    if phase == "handshake":
                        ports[0].lines.clear()
                        runner.setup_timeout = 0.01
                        with self.assertRaises(TimeoutError):
                            runner.run()
                    else:
                        with patch.object(module, "open", side_effect=OSError("disk full"), create=True):
                            with self.assertRaises(OSError):
                                runner.run()
                    self.assert_closed(runner, ports)

    def test_shutdown_is_idempotent(self):
        runner, ports, _ = self.make_runner(procedure)
        runner.shutdown()
        writes = list(ports[0].writes)
        runner.shutdown()
        self.assertEqual(ports[0].writes, writes)
        gpio.cleanup.assert_called_once()

    def test_gpio_requires_low_then_high_and_removes_detection(self):
        state = gpio.HIGH
        gpio.input.side_effect = lambda pin: state
        callback = None

        def register(pin, edge, **kwargs):
            nonlocal callback
            callback = kwargs["callback"]

        gpio.add_event_detect.side_effect = register
        transitions = iter([gpio.HIGH, gpio.LOW, gpio.HIGH])

        def control():
            nonlocal state
            state = next(transitions)
            callback(18)
            return None, "", ""

        control = Mock(side_effect=control)
        self.assertTrue(procedure_io.wait_for_start(control, 18))
        self.assertEqual(control.call_count, 3)
        gpio.remove_event_detect.assert_called_once_with(18)

    def test_gpio_pulse_is_latched_and_abort_takes_priority(self):
        for command, expected in ((None, True), ("exit", False)):
            with self.subTest(command=command):
                state = gpio.LOW
                gpio.input.side_effect = lambda pin: state
                callbacks = []
                gpio.add_event_detect.side_effect = lambda *args, **kwargs: callbacks.append(kwargs["callback"])

                def control():
                    nonlocal state
                    for state in (gpio.HIGH, gpio.LOW):
                        callbacks[0](18)
                    return command, "", ""

                self.assertEqual(procedure_io.wait_for_start(control, 18), expected)

    def test_start_pin_conflict_is_rejected_before_hardware_setup(self):
        with self.assertRaises(ValueError):
            rcs_procedure.RCS_Procedure(mtv_module.PFS_Profile([]), start_pin=15)
        gpio.PWM.assert_not_called()

    def test_serial_commands_preserve_idle_gap_and_reject_short_write(self):
        port = Port()
        before = self.clock.now
        procedure_io.send_command(port, "first")
        procedure_io.send_command(port, "second")
        self.assertAlmostEqual(self.clock.now - before, 0.04)
        with patch.object(port, "write", return_value=1), self.assertRaises(OSError):
            procedure_io.send_command(port, "third")

    def test_single_servo_rcs_checkout_logs_samples_and_checked_commands(self):
        class RelayPort(Port):
            def write(self, data):
                result = super().write(data)
                command = data.decode()
                if command == "sync status":
                    self.lines.append(b"SYNC is 0\n")
                elif command == "puiso state":
                    state = b"open" if "puiso open" in self.writes[-2:] else b"closed"
                    self.lines.append(b"VALVE puiso " + state + b"\n")
                return result

        relay, loadcell = RelayPort(), Port()
        loadcell.stream = b"rcs:1.25\n"
        with patch.dict(rcs_load_test.TEST_CONFIG, {
            "actuation_port": "/dev/relay", "loadcell_port": "/dev/loadcell",
            "servo_pin": 15, "servo_hz": 333, "pulse_min_us": 500,
            "pulse_max_us": 2500, "servo_range_deg": 180,
            "closed_deg": 10, "open_deg": 90, "valves": ("puiso",),
        }):
            args = rcs_load_test.parse_args([])
        with tempfile.TemporaryDirectory() as folder:
            args.log_dir = Path(folder)
            with patch.object(rcs_load_test.serial, "Serial", side_effect=[relay, loadcell]), \
                 patch.object(rcs_load_test, "terminal_input", side_effect=[
                     "start", "set-zero 12", "set-open 100", "valve puiso open", "servo 50", "stop"]):
                runner = rcs_load_test.RcsLoadTest(args)
                runner.run()
            self.assertTrue(relay.closed and loadcell.closed)
            self.assertEqual(len(self.pwms), 1)
            self.pwms[0].stop.assert_called_once()
            self.assertIn("puiso open", relay.writes)
            self.assertEqual(relay.writes[-1], "puiso close")
            load_log = next(Path(folder).glob("rcs-load-*.csv")).read_text()
            action_log = next(Path(folder).glob("rcs-actions-*.csv")).read_text()
            self.assertIn("time_s,rcs", load_log)
            self.assertIn(",1.25", load_log)
            self.assertIn("puiso,open,,Arduino output state verified", action_log)
            self.assertIn("servo,closed,12.0,endpoint updated", action_log)
            self.assertIn("servo,open,100.0,endpoint updated", action_log)
            self.assertIn("servo,50%,56.0,PWM command sent", action_log)
            self.assertEqual(next(Path(folder).glob("rcs-load-*.csv")).name.replace("rcs-load-", ""),
                             next(Path(folder).glob("rcs-actions-*.csv")).name.replace("rcs-actions-", ""))

    def test_single_servo_code_config_must_identify_hardware(self):
        with patch.dict(rcs_load_test.TEST_CONFIG, {
            "actuation_port": "", "loadcell_port": "", "servo_pin": None,
        }), self.assertRaises(SystemExit):
            rcs_load_test.parse_args([])

    def test_rcs_valve_state_mismatch_is_not_reported_as_verified(self):
        class RelayPort(Port):
            def write(self, data):
                result = super().write(data)
                if data == b"puiso state":
                    self.lines.append(b"VALVE puiso closed\n")
                return result

        args = rcs_load_test.parse_args([
            "--actuation-port", "/dev/relay", "--loadcell-port", "/dev/loadcell",
            "--servo-pin", "15", "--valve", "puiso",
        ])
        runner = rcs_load_test.RcsLoadTest(args)
        runner.actuation = RelayPort()
        runner.loadcell = Port()
        runner.started_at = self.clock.now
        runner.action_log = io.StringIO()
        runner.action_writer = csv.writer(runner.action_log)
        with self.assertRaisesRegex(RuntimeError, "No matching Arduino valve state"):
            runner.checked_valve("puiso", "open")
        self.assertIn("puiso,open,,unverified", runner.action_log.getvalue())

    def test_single_servo_rcs_checkout_stops_when_samples_never_start(self):
        class RelayPort(Port):
            def write(self, data):
                result = super().write(data)
                if data == b"sync status":
                    self.lines.append(b"SYNC is 0\n")
                elif data == b"puiso state":
                    self.lines.append(b"VALVE puiso closed\n")
                return result

        relay, loadcell = RelayPort(), Port()
        args = rcs_load_test.parse_args([
            "--actuation-port", "/dev/relay", "--loadcell-port", "/dev/loadcell",
            "--servo-pin", "15", "--servo-hz", "333", "--pulse-min-us", "500",
            "--pulse-max-us", "2500", "--servo-range-deg", "180",
            "--closed-deg", "10", "--open-deg", "90", "--valve", "puiso",
        ])
        with tempfile.TemporaryDirectory() as folder:
            args.log_dir = Path(folder)
            with patch.object(rcs_load_test.serial, "Serial", side_effect=[relay, loadcell]), \
                 self.assertRaisesRegex(TimeoutError, "No load-cell sample"):
                rcs_load_test.RcsLoadTest(args).run()
            self.assertTrue(relay.closed and loadcell.closed)
            self.assertEqual(relay.writes[-1], "puiso close")
            self.pwms[0].stop.assert_called_once()
            self.assertIn("puiso,close", next(Path(folder).glob("rcs-actions-*.csv")).read_text())

    def test_single_servo_blips_make_twenty_alternating_actions_in_ten_seconds(self):
        class RelayPort(Port):
            def write(self, data):
                result = super().write(data)
                if data == b"sync status":
                    self.lines.append(b"SYNC is 0\n")
                elif data == b"puiso state":
                    self.lines.append(b"VALVE puiso closed\n")
                return result

        relay, loadcell = RelayPort(), Port()
        loadcell.stream = b"rcs:2.5\n"
        with patch.dict(rcs_load_test.TEST_CONFIG, {"valves": (), "loadcells": ("rcs",)}):
            args = rcs_load_test.parse_args([
                "--actuation-port", "/dev/relay", "--loadcell-port", "/dev/loadcell",
                "--servo-pin", "15",
            ])
        with tempfile.TemporaryDirectory() as folder:
            args.log_dir = Path(folder)
            queued = deque(["start", "blips"])
            runner = rcs_load_test.RcsLoadTest(args)

            def input_for_run(**kwargs):
                if queued:
                    return queued.popleft()
                if runner.blips_index == 20 and runner.blips_started_at is None:
                    return "stop"
                return None

            with patch.object(rcs_load_test.serial, "Serial", side_effect=[relay, loadcell]), \
                 patch.object(rcs_load_test, "terminal_input", side_effect=input_for_run):
                runner.run()
            with next(Path(folder).glob("rcs-actions-*.csv")).open(newline="") as file:
                rows = list(csv.DictReader(file))
            servo_rows = [row for row in rows if row["actuator"] == "servo" and
                          row["result"] == "PWM command sent; no position feedback"]
            self.assertEqual(len(servo_rows), 20)
            self.assertEqual([row["state"] for row in servo_rows], ["open", "closed"] * 10)
            times = [float(row["time_s"]) for row in servo_rows]
            for before, after in zip(times, times[1:]):
                self.assertAlmostEqual(after - before, 0.5, delta=0.01)
            self.assertGreaterEqual(self.clock.now - runner.started_at, 10)

    def test_single_servo_logs_multiple_load_cells_on_the_same_time_column(self):
        class RelayPort(Port):
            def write(self, data):
                result = super().write(data)
                if data == b"sync status":
                    self.lines.append(b"SYNC is 0\n")
                elif data == b"puiso state":
                    self.lines.append(b"VALVE puiso closed\n")
                return result

        relay, loadcell = RelayPort(), Port()
        loadcell.stream = b"rcs:1.25 nitrous:2.5\n"
        args = rcs_load_test.parse_args([
            "--actuation-port", "/dev/relay", "--loadcell-port", "/dev/loadcell",
            "--servo-pin", "15", "--valve", "puiso",
            "--loadcell", "rcs", "--loadcell", "nitrous",
        ])
        with tempfile.TemporaryDirectory() as folder:
            args.log_dir = Path(folder)
            with patch.object(rcs_load_test.serial, "Serial", side_effect=[relay, loadcell]), \
                 patch.object(rcs_load_test, "terminal_input", return_value="stop"):
                rcs_load_test.RcsLoadTest(args).run()
            with next(Path(folder).glob("rcs-load-*.csv")).open(newline="") as file:
                rows = list(csv.reader(file))
            self.assertEqual(rows[0], ["time_s", "rcs", "nitrous"])
            self.assertEqual(rows[1][1:], ["1.25", "2.5"])
            self.assertIn("rcs setup", loadcell.writes)
            self.assertIn("nitrous setup", loadcell.writes)
            self.assertIn("rcs end", loadcell.writes)
            self.assertIn("nitrous end", loadcell.writes)

    def test_launchers_construct_current_profile_apis_without_hardware(self):
        for module in launchers:
            with self.subTest(module=module.__name__), patch.object(module, "Procedure") as factory:
                module.main()
                factory.return_value.run.assert_called_once()

    def test_mtv_rejects_valve_angles_outside_zero_to_ninety_before_moving(self):
        for direction in (-1, 1):
            mtv = mtv_module.MTV(15, 33, valve_direction=direction)
            for angle in (-0.01, 90.01, float("nan"), float("inf")):
                with self.subTest(angle=angle, direction=direction), self.assertRaises(ValueError):
                    mtv.command(angle)
        for pwm in self.pwms:
            pwm.ChangeDutyCycle.assert_not_called()

    def test_mtv_preserves_offset_across_two_to_one_valve_travel(self):
        mtv = mtv_module.MTV(15, 33, close_angle=41.5, servo2_offset=-2)
        with patch.object(mtv.servo1, "command") as lead, patch.object(mtv.servo2, "command") as follower:
            for angle in (0, 45, 90):
                mtv.command(angle)
            self.assertEqual([args[0][0] for args in lead.call_args_list], [83, 173, 263])
            self.assertEqual([args[0][0] for args in follower.call_args_list], [81, 171, 261])

    def test_mtv_rejects_calibration_without_full_stroke_before_gpio_setup(self):
        for kwargs in ({"close_angle": 100}, {"servo2_offset": -90}, {"servo2_offset": 100},
                       {"gear_ratio": 0}, {"servo_range": float("nan")},
                       {"valve_direction": 0}, {"valve_direction": float("nan")}):
            with self.subTest(kwargs=kwargs), self.assertRaises(ValueError):
                mtv_module.MTV(15, 33, **kwargs)
        with self.assertRaises(ValueError):
            mtv_module.MTV(15, 15)
        gpio.setup.assert_not_called()
        gpio.PWM.assert_not_called()

    def test_shared_calibration_defaults_reach_both_procedure_runners(self):
        with patch.object(mtv_module, "DEFAULT_CLOSE_ANGLE", 50), patch.object(mtv_module, "DEFAULT_SERVO2_OFFSET", 3):
            for module in (procedure, rcs_procedure):
                with self.subTest(module=module.__name__):
                    runner, ports, _ = self.make_runner(module)
                    self.pwms[-2].start.assert_called_once_with(runner.mtv.servo1._get_pwm(100))
                    self.pwms[-1].start.assert_called_once_with(runner.mtv.servo2._get_pwm(103))
                    runner.shutdown()

    def test_calibration_adjusts_follower_alone_then_moves_pair_with_offset(self):
        lead, follower = Mock(), Mock()
        commands = ["offset 6", "jog 2", "lead 120", "trim -1", "valve 90", "valve 0", "quit"]
        with patch.object(mtv_servo, "Servo", side_effect=[lead, follower]) as factory, \
                patch("builtins.input", side_effect=commands):
            calibrate_mtv.main(["--lead", "80", "--offset", "4"])
        self.assertEqual([call.kwargs["initial_angle"] for call in factory.call_args_list], [80, 84])
        self.assertEqual([call.args[0] for call in lead.command.call_args_list], [82, 120, 300, 120])
        self.assertEqual([call.args[0] for call in follower.command.call_args_list], [86, 88, 126, 125, 305, 125])
        lead.stop.assert_called_once()
        follower.stop.assert_called_once()
        gpio.cleanup.assert_called_once()

    def test_calibration_rejects_invalid_input_without_partial_motion_or_state_change(self):
        lead, follower = Mock(), Mock()
        commands = ["lead 350", "offset -100", "valve 91", "lead nan", "trim inf", "offset bad", "unknown 1",
                    "servo1 -1", "servo2 356", "servo1 nan", "servo2 inf", "show", "jog 1", "quit"]
        with patch.object(mtv_servo, "Servo", side_effect=[lead, follower]), patch("builtins.input", side_effect=commands):
            calibrate_mtv.main(["--lead", "80", "--offset", "10"])
        lead.command.assert_called_once_with(81)
        follower.command.assert_called_once_with(91)

    def test_independent_servo_commands_leave_the_other_servo_untouched(self):
        for command, selected in (("servo1 100.1", 0), ("servo2 100.1", 1)):
            with self.subTest(command=command):
                servos = [Mock(), Mock()]
                with patch.object(mtv_servo, "Servo", side_effect=servos), \
                        patch("builtins.input", side_effect=[command, "quit"]):
                    calibrate_mtv.main(["--lead", "80.2", "--offset", "4.3"])
                servos[selected].command.assert_called_once()
                self.assertAlmostEqual(servos[selected].command.call_args.args[0], 100.1)
                servos[1 - selected].command.assert_not_called()
                for servo in servos:
                    servo.stop.assert_called_once()

    def test_independent_servo_commands_preserve_valve_preview_and_calibration(self):
        lead, follower = Mock(), Mock()
        commands = ["valve 45", "servo1 171.5", "servo2 180.25", "valve 0", "quit"]
        output = io.StringIO()
        with patch.object(mtv_servo, "Servo", side_effect=[lead, follower]), \
                patch("builtins.input", side_effect=commands), redirect_stdout(output):
            calibrate_mtv.main(["--lead", "80", "--offset", "4"])
        self.assertEqual([call.args[0] for call in lead.command.call_args_list], [170, 171.5, 81.5])
        self.assertEqual([call.args[0] for call in follower.command.call_args_list], [174, 180.25, 90.25])
        self.assertIn("DEFAULT_CLOSE_ANGLE = 40.75", output.getvalue())
        self.assertIn("DEFAULT_SERVO2_OFFSET = 8.75", output.getvalue())

    def test_calibration_reports_zero_defaults_independently_of_preview_angle(self):
        output = io.StringIO()
        with redirect_stdout(output):
            calibrate_mtv.show(94, -2.5, 90, 2, 355)
        self.assertIn("servo 1=274, servo 2=271.5", output.getvalue())
        self.assertIn("DEFAULT_CLOSE_ANGLE = 47", output.getvalue())
        self.assertIn("DEFAULT_SERVO2_OFFSET = -2.5", output.getvalue())
        self.assertEqual(calibrate_mtv.positions(250, 0, 0, 2, 355), (250, 250))
        output = io.StringIO()
        with redirect_stdout(output):
            calibrate_mtv.show(250, 0, 0, 2, 355)
        self.assertIn("Full valve stroke exceeds servo travel", output.getvalue())

    def test_calibration_cleanup_on_partial_initialization_and_write_failure(self):
        lead, follower = Mock(), Mock()
        with patch.object(mtv_servo, "Servo", side_effect=[lead, OSError("second servo failed")]):
            with self.assertRaises(OSError):
                calibrate_mtv.main([])
        lead.stop.assert_called_once()
        gpio.cleanup.assert_called_once()
        lead.reset_mock()
        gpio.cleanup.reset_mock()
        follower.command.side_effect = OSError("servo command failed")
        with patch.object(mtv_servo, "Servo", side_effect=[lead, follower]), \
                patch("builtins.input", return_value="jog 1"):
            with self.assertRaises(OSError):
                calibrate_mtv.main([])
        lead.stop.assert_called_once()
        follower.stop.assert_called_once()
        gpio.cleanup.assert_called_once()

    def test_calibration_interrupt_stops_pwm_without_commanding_another_position(self):
        for failure in (KeyboardInterrupt, EOFError):
            with self.subTest(failure=failure):
                lead, follower = Mock(), Mock()
                with patch.object(mtv_servo, "Servo", side_effect=[lead, follower]), \
                        patch("builtins.input", side_effect=failure):
                    calibrate_mtv.main([])
                for servo in (lead, follower):
                    servo.command.assert_not_called()
                    servo.stop.assert_called_once()

    def test_calibration_rejects_invalid_startup_positions_before_servo_creation(self):
        for args in (["--lead", "350", "--offset", "10"], ["--lead", "nan"], ["--pin1", "15", "--pin2", "15"],
                     ["--direction1", "0"], ["--valve-direction", "0"], ["--lead", "200", "--valve-direction", "-1"]):
            with self.subTest(args=args), patch.object(mtv_servo, "Servo") as factory:
                with self.assertRaises(SystemExit):
                    calibrate_mtv.main(args)
                factory.assert_not_called()

    def test_mtv_and_calibration_match_for_all_direction_combinations(self):
        for direction1, direction2, servo_range, valve_direction in product((-1, 1), (-1, 1), (270, 355), (-1, 1)):
            with self.subTest(directions=(direction1, direction2, valve_direction), servo_range=servo_range):
                expected1 = (80, 170, 260) if direction1 == 1 else tuple(servo_range - a for a in (80, 170, 260))
                expected2 = (84, 174, 264) if direction2 == 1 else tuple(servo_range - a for a in (84, 174, 264))
                if valve_direction == -1:
                    expected1, expected2 = expected1[::-1], expected2[::-1]
                mtv = mtv_module.MTV(15, 32, close_angle=40, servo2_offset=4, servo_range=servo_range,
                                    servo1_direction=direction1, servo2_direction=direction2, valve_direction=valve_direction)
                self.pwms[-2].start.assert_called_once_with(mtv.servo1._get_pwm(expected1[0]))
                self.pwms[-1].start.assert_called_once_with(mtv.servo2._get_pwm(expected2[0]))
                with patch.object(mtv.servo1, "command") as lead, patch.object(mtv.servo2, "command") as follower:
                    for valve, angle1, angle2 in zip((0, 45, 90), expected1, expected2):
                        self.assertEqual(calibrate_mtv.positions(80, 4, valve, 2, servo_range, direction1, direction2, valve_direction), (angle1, angle2))
                        mtv.command(valve)
                        lead.assert_called_with(angle1)
                        follower.assert_called_with(angle2)
                mtv.stop()

    def test_mtv_rejects_invalid_directions_and_reverse_travel_before_gpio_setup(self):
        for kwargs in ({"servo1_direction": 0}, {"servo2_direction": 2},
                       {"servo1_direction": -1, "close_angle": 100},
                       {"servo2_direction": -1, "servo2_offset": 100}, {"servo2_direction": float("nan")}):
            with self.subTest(kwargs=kwargs), self.assertRaises(ValueError):
                mtv_module.MTV(15, 33, **kwargs)
        gpio.setup.assert_not_called()

    def test_direction_defaults_reach_both_runners(self):
        with patch.multiple(mtv_module, DEFAULT_CLOSE_ANGLE=50, DEFAULT_SERVO2_OFFSET=3,
                            DEFAULT_SERVO1_DIRECTION=-1, DEFAULT_SERVO2_DIRECTION=-1):
            for module in (procedure, rcs_procedure):
                with self.subTest(module=module.__name__):
                    runner, _, _ = self.make_runner(module)
                    with patch.object(runner.mtv.servo1, "command") as lead, patch.object(runner.mtv.servo2, "command") as follower:
                        runner.mtv.command(90)
                        lead.assert_called_once_with(75)
                        follower.assert_called_once_with(72)
                    runner.shutdown()

    def test_flipping_directions_at_zero_mirrors_each_servo_immediately(self):
        lead, follower = Mock(), Mock()
        commands = ["direction1 -1", "flip2", "flip1", "direction2 1", "quit"]
        with patch.object(mtv_servo, "Servo", side_effect=[lead, follower]), patch("builtins.input", side_effect=commands):
            calibrate_mtv.main([])
        self.assertEqual([call.args[0] for call in lead.command.call_args_list], [267, 88])
        self.assertEqual([call.args[0] for call in follower.command.call_args_list], [267, 88])
        self.assertEqual(self.output_path.read_text().count("# Calibration "), 1)

    def test_flips_map_independent_commands_across_entire_configured_range(self):
        for selected in (0, 1):
            for servo_range in (270, 355):
                with self.subTest(selected=selected, servo_range=servo_range):
                    servos = [Mock(), Mock()]
                    command = f"servo{selected + 1}"
                    flip = f"flip{selected + 1}"
                    commands = [flip, f"{command} 0", f"{command} {servo_range / 2}",
                                f"{command} {servo_range}", f"{command} 30", flip, flip, "quit"]
                    with patch.object(mtv_servo, "Servo", side_effect=servos), \
                            patch.object(mtv_module, "DEFAULT_SERVO_RANGE", servo_range), \
                            patch("builtins.input", side_effect=commands):
                        calibrate_mtv.main(["--defaults", "--lead", "80", "--offset", "4"])
                    expected = [servo_range - (80, 84)[selected], servo_range, servo_range / 2,
                                0, servo_range - 30, 30, servo_range - 30]
                    self.assertEqual([call.args[0] for call in servos[selected].command.call_args_list], expected)
                    servos[1 - selected].command.assert_not_called()

    def test_direction_changes_require_zero_and_valid_sign(self):
        lead, follower = Mock(), Mock()
        commands = ["direction1 0", "direction2 nan", "valve 10", "flip2", "direction1 -1", "valve 0", "flip2", "quit"]
        output = io.StringIO()
        with patch.object(mtv_servo, "Servo", side_effect=[lead, follower]), \
                patch("builtins.input", side_effect=commands), redirect_stdout(output):
            calibrate_mtv.main(["--lead", "90", "--offset", "0"])
        self.assertEqual([call.args[0] for call in lead.command.call_args_list], [110, 90])
        self.assertEqual([call.args[0] for call in follower.command.call_args_list], [110, 90, 265])
        self.assertIn("Set valve 0 before changing servo direction", output.getvalue())
        self.assertEqual(self.output_path.read_text().count("# Calibration "), 1)

    def test_paired_and_independent_adjustments_with_opposite_directions(self):
        lead, follower = Mock(), Mock()
        commands = ["jog 2", "lead 90", "valve 45", "servo1 181.5", "servo2 170.25", "valve 0", "quit"]
        with patch.object(mtv_servo, "Servo", side_effect=[lead, follower]), patch("builtins.input", side_effect=commands):
            calibrate_mtv.main(["--lead", "80", "--offset", "4", "--direction2", "-1"])
        self.assertEqual([call.args[0] for call in lead.command.call_args_list], [82, 90, 180, 181.5, 91.5])
        self.assertEqual([call.args[0] for call in follower.command.call_args_list], [269, 261, 171, 184.75, 274.75])

    def test_saved_settings_replace_and_reproduce_calibration_in_mtv(self):
        self.output_path.write_text("# Earlier calibration\n" + calibrate_mtv.settings_text(80, 4, 0, 2, 355))
        lead, follower = Mock(), Mock()
        commands = ["flip2", "valve 10", "trim -1", "save", "quit"]
        with patch.object(mtv_servo, "Servo", side_effect=[lead, follower]), patch("builtins.input", side_effect=commands):
            calibrate_mtv.main(["--lead", "90", "--offset", "0"])
        saved = self.output_path.read_text()
        self.assertNotIn("# Earlier calibration", saved)
        self.assertEqual(saved.count("# Calibration "), 1)
        self.assertIn("# Full valve stroke: valid", saved)
        values = mtv_calibration.load_calibration(self.output_path)
        self.assertEqual(values, dict(close_angle=45, servo2_offset=-1, servo1_direction=1, servo2_direction=-1,
                                     valve_direction=1, gear_ratio=2, servo_range=355, pin1=15, pin2=32, valve=10))
        mtv = mtv_module.MTV.from_calibration(self.output_path)
        with patch.object(mtv.servo1, "command") as command1, patch.object(mtv.servo2, "command") as command2:
            mtv.command(10)
            command1.assert_called_once_with(110)
            command2.assert_called_once_with(246)
        mtv.stop()

    def test_explicit_save_uses_selected_path_without_extra_motion(self):
        path = self.output_path.with_name("assembly settings.txt")
        servos = [Mock(), Mock()]
        with patch.object(mtv_servo, "Servo", side_effect=servos), patch("builtins.input", side_effect=["save", "quit"]):
            calibrate_mtv.main(["--output", str(path)])
        self.assertIn("DEFAULT_SERVO1_DIRECTION = 1", path.read_text())
        self.assertFalse(self.output_path.exists())
        for servo in servos:
            servo.command.assert_not_called()

    def test_autosaved_direction_survives_interrupt(self):
        servos = [Mock(), Mock()]
        with patch.object(mtv_servo, "Servo", side_effect=servos), patch("builtins.input", side_effect=["flip2", KeyboardInterrupt]):
            calibrate_mtv.main([])
        self.assertIn("DEFAULT_SERVO2_DIRECTION = -1", self.output_path.read_text())
        servos[1].command.assert_called_once_with(267)
        for servo in servos:
            servo.stop.assert_called_once()

    def test_startup_direction_settings_are_saved_without_interactive_changes(self):
        servos = [Mock(), Mock()]
        with patch.object(mtv_servo, "Servo", side_effect=servos) as factory, patch("builtins.input", return_value="quit"):
            calibrate_mtv.main(["--lead", "80", "--offset", "4", "--direction1", "-1", "--direction2", "-1"])
        self.assertEqual([call.kwargs["initial_angle"] for call in factory.call_args_list], [275, 271])
        saved = self.output_path.read_text()
        self.assertIn("DEFAULT_SERVO1_DIRECTION = -1", saved)
        self.assertIn("DEFAULT_SERVO2_DIRECTION = -1", saved)
        self.assertIn("DEFAULT_CLOSE_ANGLE = 40", saved)
        self.assertIn("# PWM angles: servo 1=275, servo 2=271", saved)
        self.assertIn("# Full valve stroke: valid", saved)

    def test_unwritable_output_directory_prevents_servo_initialization(self):
        with patch.object(mtv_servo, "Servo") as factory:
            with self.assertRaises(FileNotFoundError):
                calibrate_mtv.main(["--output", str(self.output_path.parent / "missing" / "calibration.txt")])
        factory.assert_not_called()

    def test_save_failure_still_stops_pwm_and_cleans_gpio(self):
        servos = [Mock(), Mock()]
        with patch.object(mtv_servo, "Servo", side_effect=servos), patch("builtins.input", return_value="jog 1"), \
                patch.object(calibrate_mtv, "save_settings", side_effect=[None, OSError("disk full")]):
            with self.assertRaises(OSError):
                calibrate_mtv.main([])
        for servo in servos:
            servo.stop.assert_called_once()
        gpio.cleanup.assert_called_once()

    def test_flipvalve_relabels_current_position_without_motion(self):
        for angle in (0, 17.25, 45, 90):
            with self.subTest(angle=angle):
                servos = [Mock(), Mock()]
                commands = [f"valve {angle}", "flipvalve", "quit"]
                with patch.object(mtv_servo, "Servo", side_effect=servos), patch("builtins.input", side_effect=commands):
                    calibrate_mtv.main(["--defaults", "--lead", "40", "--offset", "5", "--direction1", "-1", "--direction2", "-1"])
                for servo, zero in zip(servos, (315, 310)):
                    if angle == 0:
                        servo.command.assert_not_called()
                    else:
                        servo.command.assert_called_once_with(zero - 2 * angle)
                saved = self.output_path.read_text().split("# Calibration ")[-1]
                self.assertIn(f"valve={90 - angle:g};", saved)
                self.assertIn("DEFAULT_VALVE_DIRECTION = -1", saved)
                self.assertIn("DEFAULT_CLOSE_ANGLE = 20", saved)
                self.assertIn("DEFAULT_SERVO2_OFFSET = 5", saved)

    def test_flipvalve_saved_settings_swap_latest_calibration_endpoints(self):
        servos = [Mock(), Mock()]
        commands = ["flip valve", "valve 0", "valve 45", "valve 90", "quit"]
        with patch.object(mtv_servo, "Servo", side_effect=servos), patch("builtins.input", side_effect=commands):
            calibrate_mtv.main(["--lead", "40", "--offset", "5", "--direction1", "-1", "--direction2", "-1"])
        self.assertEqual([call.args[0] for call in servos[0].command.call_args_list], [135, 225, 315])
        self.assertEqual([call.args[0] for call in servos[1].command.call_args_list], [130, 220, 310])
        self.assertEqual(mtv_calibration.load_calibration(self.output_path)["valve_direction"], -1)
        mtv = mtv_module.MTV.from_calibration(self.output_path)
        self.pwms[-2].start.assert_called_once_with(mtv.servo1._get_pwm(135))
        self.pwms[-1].start.assert_called_once_with(mtv.servo2._get_pwm(130))
        with patch.object(mtv.servo1, "command") as lead, patch.object(mtv.servo2, "command") as follower:
            for angle in (0, 45, 90):
                mtv.command(angle)
            self.assertEqual([call.args[0] for call in lead.call_args_list], [135, 225, 315])
            self.assertEqual([call.args[0] for call in follower.call_args_list], [130, 220, 310])
        mtv.stop()

    def test_flipvalve_twice_restores_mapping_without_motion(self):
        servos = [Mock(), Mock()]
        with patch.object(mtv_servo, "Servo", side_effect=servos), \
                patch("builtins.input", side_effect=["flipvalve", "flip valve", "quit"]):
            calibrate_mtv.main([])
        for servo in servos:
            servo.command.assert_not_called()
        saved = self.output_path.read_text().split("# Calibration ")[-1]
        self.assertIn("valve=0;", saved)
        self.assertIn("DEFAULT_VALVE_DIRECTION = 1", saved)

    def test_reversed_valve_startup_and_independent_adjustments(self):
        servos = [Mock(), Mock()]
        commands = ["valve 60", "servo1 110", "servo2 112", "jog 1", "trim -1", "valve 0", "quit"]
        with patch.object(mtv_servo, "Servo", side_effect=servos) as factory, patch("builtins.input", side_effect=commands):
            calibrate_mtv.main(["--lead", "40", "--offset", "5", "--direction1", "-1", "--direction2", "-1",
                                "--valve-direction", "-1"])
        self.assertEqual([call.kwargs["initial_angle"] for call in factory.call_args_list], [135, 130])
        self.assertEqual([call.args[0] for call in servos[0].command.call_args_list], [255, 245, 244, 124])
        self.assertEqual([call.args[0] for call in servos[1].command.call_args_list], [250, 243, 242, 243, 123])
        saved = self.output_path.read_text().split("# Calibration ")[-1]
        self.assertIn("DEFAULT_CLOSE_ANGLE = 25.5", saved)
        self.assertIn("DEFAULT_SERVO2_OFFSET = 1", saved)
        self.assertIn("DEFAULT_VALVE_DIRECTION = -1", saved)
        self.assertIn("servo 1=231, servo 2=232", saved)
        self.assertIn("# PWM angles: servo 1=124, servo 2=123", saved)

    def test_valve_direction_default_reaches_startup_and_shutdown_in_both_runners(self):
        with patch.multiple(mtv_module, DEFAULT_CLOSE_ANGLE=20, DEFAULT_SERVO2_OFFSET=5,
                            DEFAULT_SERVO1_DIRECTION=-1, DEFAULT_SERVO2_DIRECTION=-1, DEFAULT_VALVE_DIRECTION=-1):
            for module in (procedure, rcs_procedure):
                with self.subTest(module=module.__name__):
                    runner, _, _ = self.make_runner(module)
                    self.pwms[-2].start.assert_called_once_with(runner.mtv.servo1._get_pwm(135))
                    self.pwms[-1].start.assert_called_once_with(runner.mtv.servo2._get_pwm(130))
                    with patch.object(runner.mtv.servo1, "command") as lead, patch.object(runner.mtv.servo2, "command") as follower:
                        runner.shutdown()
                        lead.assert_called_once_with(135)
                        follower.assert_called_once_with(130)

    def test_calibration_restart_restores_latest_settings_and_position(self):
        self.output_path.write_text(CALIBRATION)
        servos = [Mock(), Mock()]
        with patch.object(mtv_servo, "Servo", side_effect=servos) as factory, \
                patch("builtins.input", side_effect=["trim 2", "valve 40", "save", "quit"]):
            calibrate_mtv.main([])
        self.assertEqual([call.args[0] for call in factory.call_args_list], [15, 32])
        self.assertEqual([call.kwargs["initial_angle"] for call in factory.call_args_list], [195, 190])
        self.assertEqual(self.output_path.read_text().count("# Calibration "), 1)
        with patch.object(mtv_servo, "Servo", side_effect=[Mock(), Mock()]) as factory, \
                patch("builtins.input", return_value="quit"):
            calibrate_mtv.main([])
        self.assertEqual([call.kwargs["initial_angle"] for call in factory.call_args_list], [215, 208])
        saved = mtv_calibration.load_calibration(self.output_path)
        self.assertEqual(saved["servo2_offset"], 7)
        self.assertEqual(saved["valve"], 40)
        self.assertEqual(saved["valve_direction"], -1)

    def test_calibration_import_preserves_source_and_applies_explicit_overrides(self):
        source = self.output_path.with_name("import.txt")
        original = CALIBRATION.replace("DEFAULT_GEAR_RATIO = 2", "DEFAULT_GEAR_RATIO = 1").replace(
            "DEFAULT_SERVO_RANGE = 355", "DEFAULT_SERVO_RANGE = 270")
        source.write_text(original)
        with patch.object(mtv_servo, "Servo", side_effect=[Mock(), Mock()]) as factory, \
                patch("builtins.input", return_value="quit"):
            calibrate_mtv.main(["--input", str(source), "--lead", "45", "--pin1", "33", "--valve", "0"])
        self.assertEqual(source.read_text(), original)
        self.assertEqual([call.args[0] for call in factory.call_args_list], [33, 32])
        self.assertEqual([call.kwargs["range"] for call in factory.call_args_list], [270, 270])
        self.assertEqual([call.kwargs["initial_angle"] for call in factory.call_args_list], [135, 130])
        saved = mtv_calibration.load_calibration(self.output_path)
        self.assertEqual(saved["gear_ratio"], 1)
        self.assertEqual(saved["close_angle"], 45)
        self.assertEqual(saved["servo1_direction"], -1)

    def test_loader_accepts_latest_legacy_block_and_consolidates_on_save(self):
        legacy = "\n".join(line for line in CALIBRATION.splitlines() if not line.startswith(
            ("DEFAULT_SERVO1_PIN", "DEFAULT_SERVO2_PIN", "CALIBRATION_VALVE_ANGLE", "DEFAULT_VALVE_DIRECTION")))
        self.output_path.write_text(CALIBRATION + "\n" + legacy)
        saved = mtv_calibration.load_calibration(self.output_path)
        self.assertEqual((saved["pin1"], saved["pin2"], saved["valve"], saved["valve_direction"]), (15, 32, 30, 1))
        with patch.object(mtv_servo, "Servo", side_effect=[Mock(), Mock()]) as factory, \
                patch("builtins.input", return_value="quit"):
            calibrate_mtv.main([])
        self.assertEqual([call.kwargs["initial_angle"] for call in factory.call_args_list], [255, 250])
        self.assertEqual(self.output_path.read_text().count("# Calibration "), 1)

    def test_invalid_saved_calibrations_fail_before_motion_or_overwrite(self):
        bad_files = [
            "", CALIBRATION + "\n# Calibration incomplete\nDEFAULT_CLOSE_ANGLE = 10\n",
            CALIBRATION.replace("DEFAULT_SERVO1_DIRECTION = -1", "DEFAULT_SERVO1_DIRECTION = 0"),
            CALIBRATION.replace("DEFAULT_SERVO1_PIN = 15", "DEFAULT_SERVO1_PIN = 32"),
            CALIBRATION.replace("DEFAULT_SERVO1_PIN = 15", "DEFAULT_SERVO1_PIN = True"),
            CALIBRATION.replace("CALIBRATION_VALVE_ANGLE = 30", "CALIBRATION_VALVE_ANGLE = 91"),
            CALIBRATION.replace("DEFAULT_GEAR_RATIO = 2", "DEFAULT_GEAR_RATIO = 0"),
            CALIBRATION.replace("DEFAULT_SERVO_RANGE = 355", "DEFAULT_SERVO_RANGE = 1e999"),
            CALIBRATION + "DEFAULT_CLOSE_ANGLE = 40\n",
            CALIBRATION + "UNKNOWN_SETTING = 1\n",
            CALIBRATION.replace("DEFAULT_CLOSE_ANGLE = 20", "DEFAULT_CLOSE_ANGLE = float('20')"),
        ]
        for content in bad_files:
            with self.subTest(content=content):
                self.output_path.write_text(content)
                with patch.object(mtv_servo, "Servo") as factory, self.assertRaises(SystemExit):
                    calibrate_mtv.main([])
                factory.assert_not_called()
                self.assertEqual(self.output_path.read_text(), content)

    def test_missing_explicit_input_fails_before_motion_or_output(self):
        with patch.object(mtv_servo, "Servo") as factory, self.assertRaises(SystemExit):
            calibrate_mtv.main(["--input", str(self.output_path.with_name("missing.txt"))])
        factory.assert_not_called()
        self.assertFalse(self.output_path.exists())

    def test_defaults_option_ignores_saved_file_and_replaces_it(self):
        self.output_path.write_text("invalid calibration")
        with patch.object(mtv_servo, "Servo", side_effect=[Mock(), Mock()]) as factory, \
                patch("builtins.input", return_value="quit"):
            calibrate_mtv.main(["--defaults"])
        self.assertEqual([call.kwargs["initial_angle"] for call in factory.call_args_list], [88, 88])
        self.assertEqual(mtv_calibration.load_calibration(self.output_path)["valve_direction"], 1)

    def test_failed_atomic_save_preserves_last_calibration_and_stops_servos(self):
        self.output_path.write_text(CALIBRATION)
        replace = mtv_calibration.os.replace
        previous = []

        def command(prompt):
            previous.append(self.output_path.read_bytes())
            return "trim 1"

        def replace_once(source, destination):
            if previous:
                raise OSError("save failed")
            replace(source, destination)

        servos = [Mock(), Mock()]
        with patch.object(mtv_calibration.os, "replace", side_effect=replace_once), \
                patch.object(mtv_servo, "Servo", side_effect=servos), patch("builtins.input", side_effect=command):
            with self.assertRaises(OSError):
                calibrate_mtv.main([])
        self.assertEqual(self.output_path.read_bytes(), previous[0])
        self.assertEqual(list(self.output_path.parent.iterdir()), [self.output_path])
        for servo in servos:
            servo.stop.assert_called_once()
        gpio.cleanup.assert_called_once()

    def test_mtv_from_file_uses_closed_position_and_saved_pins_for_shutdown(self):
        self.output_path.write_text(CALIBRATION)
        mtv = mtv_module.MTV.from_calibration(self.output_path)
        self.assertEqual([call.args[0] for call in gpio.PWM.call_args_list], [15, 32])
        self.pwms[-2].start.assert_called_once_with(mtv.servo1._get_pwm(135))
        self.pwms[-1].start.assert_called_once_with(mtv.servo2._get_pwm(130))
        mtv.command(90)
        procedure_io.shutdown(mtv)
        self.pwms[-2].ChangeDutyCycle.assert_called_with(mtv.servo1._get_pwm(135))
        self.pwms[-1].ChangeDutyCycle.assert_called_with(mtv.servo2._get_pwm(130))
        for pwm in self.pwms:
            pwm.stop.assert_called_once()
        self.assertEqual(self.output_path.read_text(), CALIBRATION)

    def test_mtv_from_file_rejects_incomplete_valve_travel_before_gpio(self):
        self.output_path.write_text(CALIBRATION.replace("DEFAULT_CLOSE_ANGLE = 20", "DEFAULT_CLOSE_ANGLE = 100"))
        with self.assertRaises(ValueError):
            mtv_module.MTV.from_calibration(self.output_path)
        gpio.setup.assert_not_called()
        gpio.PWM.assert_not_called()


if __name__ == "__main__":
    unittest.main()
