import importlib
import io
import sys
import unittest
from collections import deque
from contextlib import ExitStack, redirect_stderr, redirect_stdout
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
    import calibrate_mtv
    launchers = [importlib.import_module(name) for name in ("hotfire", "coldflow", "kaboom_test")]


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
        modules = (mtv_module, mtv_servo, procedure, procedure_io, rcs_procedure, calibrate_mtv, *launchers)
        self.stack.enter_context(patch.dict(sys.modules, {
            "Jetson": jetson, "Jetson.GPIO": gpio, **{module.__name__: module for module in modules}
        }))
        self.stack.enter_context(redirect_stdout(io.StringIO()))
        self.stack.enter_context(redirect_stderr(io.StringIO()))
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

    def test_launchers_construct_current_profile_apis_without_hardware(self):
        for module in launchers:
            with self.subTest(module=module.__name__), patch.object(module, "Procedure") as factory:
                module.main()
                factory.return_value.run.assert_called_once()

    def test_mtv_rejects_valve_angles_outside_zero_to_ninety_before_moving(self):
        mtv = mtv_module.MTV(15, 33)
        for angle in (-0.01, 90.01, float("nan"), float("inf")):
            with self.subTest(angle=angle), self.assertRaises(ValueError):
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
                       {"gear_ratio": 0}, {"servo_range": float("nan")}):
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
        commands = ["lead 350", "offset -100", "valve 91", "lead nan", "trim inf", "offset bad", "unknown 1", "show", "jog 1", "quit"]
        with patch.object(mtv_servo, "Servo", side_effect=[lead, follower]), patch("builtins.input", side_effect=commands):
            calibrate_mtv.main(["--lead", "80", "--offset", "10"])
        lead.command.assert_called_once_with(81)
        follower.command.assert_called_once_with(91)

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
        for args in (["--lead", "350", "--offset", "10"], ["--lead", "nan"], ["--pin1", "15", "--pin2", "15"]):
            with self.subTest(args=args), patch.object(mtv_servo, "Servo") as factory:
                with self.assertRaises(SystemExit):
                    calibrate_mtv.main(args)
                factory.assert_not_called()


if __name__ == "__main__":
    unittest.main()
