import ast
import math
import os
import re
import tempfile
from contextlib import contextmanager
from pathlib import Path

DEFAULT_CALIBRATION = Path(__file__).with_name("mtv_calibration.txt")

FIELDS = {
    "DEFAULT_CLOSE_ANGLE": "close_angle",
    "DEFAULT_SERVO2_OFFSET": "servo2_offset",
    "DEFAULT_SERVO1_DIRECTION": "servo1_direction",
    "DEFAULT_SERVO2_DIRECTION": "servo2_direction",
    "DEFAULT_GEAR_RATIO": "gear_ratio",
    "DEFAULT_SERVO_RANGE": "servo_range",
    "DEFAULT_VALVE_DIRECTION": "valve_direction",
    "DEFAULT_SERVO1_PIN": "pin1",
    "DEFAULT_SERVO2_PIN": "pin2",
    "CALIBRATION_VALVE_ANGLE": "valve",
}


def load_calibration(path=DEFAULT_CALIBRATION):
    block = re.split(r"(?m)^# Calibration .*?$", Path(path).read_text(encoding="utf-8"))[-1]
    values = {}
    try:
        for node in ast.parse(block).body:
            if not isinstance(node, ast.Assign) or len(node.targets) != 1 or not isinstance(node.targets[0], ast.Name):
                raise ValueError("Expected numeric calibration assignments")
            name = node.targets[0].id
            if name not in FIELDS or FIELDS[name] in values:
                raise ValueError(f"Unknown or duplicate calibration setting: {name}")
            value = ast.literal_eval(node.value)
            if type(value) not in (int, float) or not math.isfinite(value):
                raise ValueError(f"Calibration setting must be a finite number: {name}")
            values[FIELDS[name]] = value
        required = {"close_angle", "servo2_offset", "servo1_direction", "servo2_direction", "gear_ratio", "servo_range"}
        if not required <= values.keys():
            raise ValueError(f"Missing calibration settings: {', '.join(sorted(required - values.keys()))}")
        pins = re.search(r"(?m)^# BOARD pins: servo1=(\d+), servo2=(\d+)\s*$", block)
        if "# BOARD pins:" in block and pins is None:
            raise ValueError("Invalid BOARD pin metadata")
        preview = re.search(r"(?m)^# Logical angles:.*\bvalve=([^;\s]+)", block)
        values.setdefault("pin1", int(pins[1]) if pins else 15)
        values.setdefault("pin2", int(pins[2]) if pins else 32)
        values.setdefault("valve", float(preview[1]) if preview else 0)
        values.setdefault("valve_direction", 1)
        if any(type(values[key]) is not int or not 1 <= values[key] <= 40 for key in ("pin1", "pin2")) or values["pin1"] == values["pin2"]:
            raise ValueError("Calibration requires distinct BOARD pins from 1 to 40")
        if any(values[key] not in (-1, 1) for key in ("servo1_direction", "servo2_direction", "valve_direction")):
            raise ValueError("Calibration directions must be +1 or -1")
        if values["gear_ratio"] <= 0 or values["servo_range"] <= 0 or not 0 <= values["valve"] <= 90:
            raise ValueError("Invalid calibration range, gear ratio, or valve angle")
    except (SyntaxError, TypeError, ValueError, OverflowError) as error:
        raise ValueError(f"Invalid calibration file {path}: {error}") from error
    return values


@contextmanager
def calibration_writer(path):
    path = Path(path)
    if path.is_dir():
        raise IsADirectoryError(path)
    with tempfile.TemporaryDirectory(prefix=f".{path.name}.", dir=path.parent) as directory:
        pending = Path(directory) / "calibration.txt"

        def write(text):
            with pending.open("w", encoding="utf-8") as file:
                file.write(text)
                file.flush()
                os.fsync(file.fileno())
            os.replace(pending, path)

        yield write
