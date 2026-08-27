#include <Arduino.h>
#include "HX711.h"

const int OMV = 2;
const int OVENT = 3;
const int PUISO = 9;
const int IGV = 7;
const int OFILL = 10;
const int LFVENT = 11;
const int PUFILL = 8;
const int OISO_OPEN = 4;
const int OISO_CLOSE = 5;
const int PUMV = 12;  
const int PUVENT = 6;
const int KABOOM = 13;
const int SYNC = A0;

// Loadcell Arduino
const int ENGINE_LDCL_DOUT = 2;
const int ENGINE_LDCL_SCK = 3;
const int NITROUS_LDCL_DOUT = 4;
const int NITROUS_LDCL_SCK = 5;
const int RCS_LDCL_DOUT = 6;
const int RCS_LDCL_SCK = 7;
 
//const bool USE_LOADCELL = true;
const bool USE_LOADCELL = false;
bool engine_loadcell_send = false;
bool nitrous_loadcell_send = false;
bool rcs_loadcell_send = false;
const bool debug = false;

HX711 engine_loadcell;
HX711 nitrous_loadcell;
HX711 rcs_loadcell;

enum Devices {
  NONE,
  SHUTDOWN,
  RESET,
  OMV_DEVICE,
  OVENT_DEVICE,
  ENGINE_LDCL_DEVICE,
  NITROUS_LDCL_DEVICE,
  RCS_LDCL_DEVICE,
  PUISO_DEVICE,
  IGV_DEVICE,
  OFILL_DEVICE,
  LFVENT_DEVICE,
  PUFILL_DEVICE,
  OISO_DEVICE,
  PUMV_DEVICE,
  PUVENT_DEVICE,
  KABOOM_DEVICE,
  SYNC_DEVICE,
};

struct Command {
  Devices device;
  String action;
};

enum ProcedureType {
  COLDFLOW,
  HOTFIRE,
};

struct Procedure {
  ProcedureType procedure;
};

Procedure proc;

void setup() {
  // put your setup code here, to run once:  
  if (USE_LOADCELL == false) {
    pinMode(OMV, OUTPUT);
    pinMode(OVENT, OUTPUT);
    pinMode(PUISO, OUTPUT);
    pinMode(IGV, OUTPUT);
    pinMode(OFILL, OUTPUT);
    pinMode(LFVENT, OUTPUT);
    pinMode(PUFILL, OUTPUT);
    pinMode(OISO_OPEN, OUTPUT);
    pinMode(OISO_CLOSE, OUTPUT);
    pinMode(PUMV, OUTPUT);
    pinMode(PUVENT, OUTPUT);
    pinMode(KABOOM, OUTPUT);
    pinMode(SYNC, OUTPUT);
    
    proc.procedure = COLDFLOW;
    setup_starting_states(proc);
    
  } else {
    pinMode(ENGINE_LDCL_DOUT, OUTPUT);
    pinMode(ENGINE_LDCL_SCK, OUTPUT);
    pinMode(NITROUS_LDCL_DOUT, OUTPUT);
    pinMode(NITROUS_LDCL_SCK, OUTPUT);
    pinMode(RCS_LDCL_DOUT, OUTPUT);
    pinMode(RCS_LDCL_SCK, OUTPUT);
  }
  
  
  
  Serial.begin(115200);
  Serial.setTimeout(10);
  while (!Serial) { ; }
  
  Serial.println("connected");
}

void setup_starting_states(Procedure &proc) {
  switch (proc.procedure) { 
    case HOTFIRE:
      digitalWrite(OMV, LOW);
      digitalWrite(PUISO, LOW);
      digitalWrite(IGV, LOW);
      digitalWrite(OFILL, LOW);
      digitalWrite(LFVENT, LOW);
      digitalWrite(PUFILL, LOW);
      digitalWrite(OISO_CLOSE, HIGH);
      digitalWrite(OISO_OPEN, LOW);
      digitalWrite(PUMV, LOW);
      digitalWrite(KABOOM, LOW);
      digitalWrite(OVENT, HIGH);
      digitalWrite(PUVENT, HIGH);
      digitalWrite(SYNC, LOW);
    break;

    case COLDFLOW:
      digitalWrite(OMV, LOW);
      digitalWrite(PUISO, LOW);
      digitalWrite(IGV, LOW);
      digitalWrite(OFILL, LOW);
      digitalWrite(LFVENT, LOW);
      digitalWrite(PUFILL, LOW);
      digitalWrite(OISO_CLOSE, HIGH);
      digitalWrite(OISO_OPEN, LOW);
      digitalWrite(PUMV, LOW);
      digitalWrite(KABOOM, LOW);
      digitalWrite(OVENT, HIGH);
      digitalWrite(PUVENT, HIGH);
      digitalWrite(SYNC, LOW);
    break;
  }
}

void parse_command(Command &command) {
  if (Serial.available() != 0) {
    String input = Serial.readString();
    int space_index = input.indexOf(' ');

    if (debug) {
      Serial.println("command received");
    }
    
    String temp_device = input.substring(0, space_index);

    if (USE_LOADCELL == false) {
      // if else for string to enum
      if (temp_device.equals("omv")) {
        command.device = OMV_DEVICE;
      } else if (temp_device.equals("ovent")) {
        command.device = OVENT_DEVICE;
      } else if (temp_device.equals("puiso")) {
        command.device = PUISO_DEVICE;
      } else if (temp_device.equals("igv")) {
        command.device = IGV_DEVICE;
      } else if (temp_device.equals("ofill")) {
        command.device = OFILL_DEVICE;
      } else if (temp_device.equals("lfvent")) {
        command.device = LFVENT_DEVICE;
      } else if (temp_device.equals("pufill")) {
        command.device = PUFILL_DEVICE;
      } else if (temp_device.equals("oiso")) {
        command.device = OISO_DEVICE;
      } else if (temp_device.equals("pumv")) {
        command.device = PUMV_DEVICE;
      } else if (temp_device.equals("puvent")) {
        command.device = PUVENT_DEVICE;
      } else if (temp_device.equals("shutdown")) {
        command.device = SHUTDOWN;
      } else if (temp_device.equals("kaboom")) {
        command.device = KABOOM_DEVICE;
      } else if (temp_device.equals("sync")) {
        command.device = SYNC_DEVICE;
      } else if (temp_device.equals("reset")) {
        command.device = RESET;
      } else {
        command.device = NONE;
      }
    } else {
      if (temp_device.equals("engine")) {
        command.device = ENGINE_LDCL_DEVICE;
      } else if (temp_device.equals("nitrous")) {
        command.device = NITROUS_LDCL_DEVICE;
      } else if (temp_device.equals("rcs")) {
        command.device = RCS_LDCL_DEVICE;
      } else {
        command.device = NONE;
      }
    }

    
    command.action = input.substring(space_index + 1);

    while (Serial.available() > 0) {
      Serial.read();
    }
  } else {
    command.device = NONE;
    command.action = "none";
  }
}

void setup_loadcell(HX711 &loadcell, String name, const int dout_pin, const int sck_pin, int scale) {
  loadcell.begin(dout_pin, sck_pin);
  loadcell.set_scale(scale);
  loadcell.tare();
  while (!loadcell.is_ready()) { ; }
  String base = "";
  const char* message = " loadcell ready";
  String full_message = base + name + message;
  Serial.println(full_message);
}

String get_loadcell_units(HX711 &loadcell, String name) {
  String message = name;
  message += ":";
  message += loadcell.get_units();

  return message;
}

void loop() {
  Command command;
  parse_command(command);

  // For only ovent and puvent have HIGH == closed and LOW == open
  // Every other valve is HIGH == open and LOW == closed
  switch (command.device) {
    case NONE:
      break;
    
    case SHUTDOWN:
      if (command.action.equals("begin")) {
        if (debug) {
          Serial.println("Beginning shutdown procedure");
        }
        digitalWrite(OFILL, LOW);
        digitalWrite(IGV, LOW);
      }
      break;

    case RESET:
      if (command.action.equals("all")) {
        if (debug) {
          Serial.println("Resetting all to low");
        }
        digitalWrite(OMV, LOW);
        digitalWrite(PUISO, LOW);
        digitalWrite(IGV, LOW);
        digitalWrite(OFILL, LOW);
        digitalWrite(LFVENT, LOW);
        digitalWrite(PUFILL, LOW);
        digitalWrite(OISO_CLOSE, HIGH);
        digitalWrite(OISO_OPEN, LOW);
        digitalWrite(PUMV, LOW);
        digitalWrite(KABOOM, LOW);
        digitalWrite(SYNC, LOW);
        // Set these below to high if you want them to close too
        digitalWrite(OVENT, LOW);
        digitalWrite(PUVENT, LOW);
      } else if (command.action.equals("default")) {
        setup_starting_states(proc);
      }
      
    case OMV_DEVICE:
      if (command.action.equals("open")) {
        if (debug) {
          Serial.println("Opening OMV");
        }
        digitalWrite(OMV, HIGH);
      } else if (command.action.equals("close")) {
        if (debug) {
          Serial.println("Closing OMV");
        }
        digitalWrite(OMV, LOW);
      } else if (command.action.equals("status")) {
        int omv_state = digitalRead(OMV);
        String omv_message = "OMV is " + String(omv_state);
        Serial.println(omv_message);
      }
      break;
      
    case OVENT_DEVICE:
      if (command.action.equals("open")) {
        if (debug) {
          Serial.println("Opening OVENT");
        }
        digitalWrite(OVENT, LOW);
      } else if (command.action.equals("close")) {
        if (debug) {
          Serial.println("Closing OVENT");
        }
        digitalWrite(OVENT, HIGH);
      } else if (command.action.equals("status")) {
        int ovent_state = digitalRead(OVENT);
        String ovent_message = "OVENT is " + String(ovent_state);
        Serial.println(ovent_message);
      }
      break;
     
    case ENGINE_LDCL_DEVICE:
      if (command.action.equals("setup")) { 
        if (debug) {
          Serial.println("Setting up engine loadcell");
        }
        setup_loadcell(engine_loadcell, "engine", ENGINE_LDCL_DOUT, ENGINE_LDCL_SCK, 1300);
      } else if (command.action.equals("begin")) {
        if (debug) {
          Serial.println("Beginning engine loadcell data");
        }
        engine_loadcell_send = true;
      } else if (command.action.equals("end")) {
        if (debug) {
          Serial.println("Ending engine loadcell data");
        }
        engine_loadcell_send = false;      
      } else if (command.action.equals("read")) {
//        Serial.println("Loadcell read placeholder");
        Serial.println(engine_loadcell.get_units(10));
      }
      break;
     
    case NITROUS_LDCL_DEVICE:
      if (command.action.equals("setup")) { 
        if (debug) {
          Serial.println("Setting up nitrous loadcell");
        }
        setup_loadcell(nitrous_loadcell, "nitrous", NITROUS_LDCL_DOUT, NITROUS_LDCL_SCK, 2120);
      } else if (command.action.equals("begin")) {
        if (debug) {
          Serial.println("Beginning nitrous loadcell data");
        }
        nitrous_loadcell_send = true;
      } else if (command.action.equals("end")) {
        if (debug) {
          Serial.println("Ending nitrous loadcell data");
        }
        nitrous_loadcell_send = false;
      } else if (command.action.equals("read")) {
//        Serial.println("Loadcell read placeholder");
        Serial.println(nitrous_loadcell.get_units(10));
      }
      break;
     
    case RCS_LDCL_DEVICE:
      if (command.action.equals("setup")) { 
        if (debug) {
          Serial.println("Setting up rcs loadcell");
        }
        setup_loadcell(rcs_loadcell, "rcs", RCS_LDCL_DOUT, RCS_LDCL_SCK, 7550);
      } else if (command.action.equals("begin")) {
        if (debug) {
          Serial.println("Beginning rcs loadcell data");
        }
        rcs_loadcell_send = true;
      } else if (command.action.equals("end")) {
        if (debug) {
          Serial.println("Ending rcs loadcell data");
        }
        rcs_loadcell_send = false;
      } else if (command.action.equals("read")) {
//        Serial.println("Loadcell read placeholder");
        Serial.println(rcs_loadcell.get_units(10));
      }
      break;

     case PUISO_DEVICE:
      if (command.action.equals("open")) {
        if (debug) {
          Serial.println("Opening PUISO");
        }
        digitalWrite(PUISO, HIGH);
      } else if (command.action.equals("close")) {
        if (debug) {
          Serial.println("Closing PUISO");
        }
        digitalWrite(PUISO, LOW);
      } else if (command.action.equals("status")) {
        int puiso_state = digitalRead(PUISO);
        String puiso_message = "PUISO is " + String(puiso_state);
        Serial.println(puiso_message);
      }
      break;

     case IGV_DEVICE:
      if (command.action.equals("open")) {
        if (debug) {
          Serial.println("Opening IGV");
        }
        digitalWrite(IGV, HIGH);
      } else if (command.action.equals("close")) {
        if (debug) {
          Serial.println("Closing IGV");
        }
        digitalWrite(IGV, LOW);
      } else if (command.action.equals("status")) {
        int igv_state = digitalRead(IGV);
        String igv_message = "IGV is " + String(igv_state);
        Serial.println(igv_message);
      }
      break;

     case OFILL_DEVICE:
      if (command.action.equals("open")) {
        if (debug) {
          Serial.println("Opening OFILL");
        }
        digitalWrite(OFILL, HIGH);
      } else if (command.action.equals("close")) {
        if (debug) {
          Serial.println("Closing OFILL");
        }
        digitalWrite(OFILL, LOW);
      } else if (command.action.equals("status")) {
        int ofill_state = digitalRead(OFILL);
        String ofill_message = "OFILL is " + String(ofill_state);
        Serial.println(ofill_message);
      }
      break;

     case LFVENT_DEVICE:
      if (command.action.equals("open")) {
        if (debug) {
          Serial.println("Opening LFVENT");
        }
        digitalWrite(LFVENT, HIGH);
      } else if (command.action.equals("close")) {
        if (debug) {
          Serial.println("Closing LFVENT");
        }
        digitalWrite(LFVENT, LOW);
      } else if (command.action.equals("status")) {
        int lfvent_state = digitalRead(LFVENT);
        String lfvent_message = "LFVENT is " + String(lfvent_state);
        Serial.println(lfvent_message);
      }
      break;

     case PUFILL_DEVICE:
      if (command.action.equals("open")) {
        if (debug) {
          Serial.println("Opening PUFILL");
        }
        digitalWrite(PUFILL, HIGH);
      } else if (command.action.equals("close")) {
        if (debug) {
          Serial.println("Closing PUFILL");
        }
        digitalWrite(PUFILL, LOW);
      } else if (command.action.equals("status")) {
        int pufill_state = digitalRead(PUFILL);
        String pufill_message = "PUFILL is " + String(pufill_state);
        Serial.println(pufill_message);
      }
      break;

     case OISO_DEVICE:
      if (command.action.equals("open")) {
        if (debug) {
          Serial.println("Opening OISO");
        }
        digitalWrite(OISO_OPEN, HIGH);
        digitalWrite(OISO_CLOSE, LOW);
      } else if (command.action.equals("close")) {
        if (debug) {
          Serial.println("Closing OISO");
        }
        digitalWrite(OISO_OPEN, LOW);
        digitalWrite(OISO_CLOSE, HIGH);
      } else if (command.action.equals("status")) {
        int oiso_open_state = digitalRead(OISO_OPEN);
        int oiso_close_state = digitalRead(OISO_CLOSE);
        String oiso_message = "OISO_OPEN is " + String(oiso_open_state) + "\nOISO_CLOSE is " + String(oiso_close_state);
        Serial.println(oiso_message);
      }
      break;

     case PUMV_DEVICE:
      if (command.action.equals("open")) {
        if (debug) {
          Serial.println("Opening PUMV");
        }
        digitalWrite(PUMV, HIGH);
      } else if (command.action.equals("close")) {
        if (debug) {
          Serial.println("Closing PUMV");
        }
        digitalWrite(PUMV, LOW);
      } else if (command.action.equals("status")) {
        int pumv_state = digitalRead(PUMV);
        String pumv_message = "PUMV is " + String(pumv_state);
        Serial.println(pumv_message);
      }
      break;

     case PUVENT_DEVICE:
      if (command.action.equals("open")) {
        if (debug) {
          Serial.println("Opening PUVENT");
        }
        digitalWrite(PUVENT, LOW);
      } else if (command.action.equals("close")) {
        if (debug) {
          Serial.println("Closing PUVENT");
        }
        digitalWrite(PUVENT, HIGH);
      } else if (command.action.equals("status")) {
        int puvent_state = digitalRead(PUVENT);
        String puvent_message = "PUVENT is " + String(puvent_state);
        Serial.println(puvent_message);
      }
      break;

     case KABOOM_DEVICE:
      if (command.action.equals("start")) {
        if (debug) {
          Serial.println("Starting KABOOM");
        }
        digitalWrite(KABOOM, HIGH);
      } else if (command.action.equals("end")) {
        if (debug) {
          Serial.println("Ending KABOOM");
        }
        digitalWrite(KABOOM, LOW);
      } else if (command.action.equals("status")) {
        int kaboom_state = digitalRead(KABOOM);
        String kaboom_message = "KABOOM is " + String(kaboom_state);
        Serial.println(kaboom_message);
      }
      break;

     case SYNC_DEVICE:
      if (command.action.equals("high")) {
        if (debug) {
          Serial.println("Setting SYNC to HIGH");
        }
        digitalWrite(SYNC, HIGH);
      } else if (command.action.equals("low")) {
        if (debug) {
          Serial.println("Setting SYNC to LOW");
        }
        digitalWrite(SYNC, LOW);
      } else if (command.action.equals("status")) {
        int sync_state = digitalRead(SYNC);
        String sync_message = "SYNC is " + String(sync_state);
        Serial.println(sync_message);
      }
      break;

     default:
      break;
  }

  if (engine_loadcell_send || nitrous_loadcell_send || rcs_loadcell_send) {
    String loadcell_message = "";
    if (engine_loadcell_send) {
      loadcell_message += get_loadcell_units(engine_loadcell, "engine");
      loadcell_message += " ";
    }
    if (nitrous_loadcell_send) {
      loadcell_message += get_loadcell_units(nitrous_loadcell, "nitrous");
      loadcell_message += " ";
    }
    if (rcs_loadcell_send) {
      loadcell_message += get_loadcell_units(rcs_loadcell, "rcs");
      loadcell_message += " ";
    }
    loadcell_message.trim();
    Serial.println(loadcell_message);
  }

//  if (engine_loadcell_send && nitrous_loadcell_send) {
//    String loadcell_message = get_loadcell_units(engine_loadcell, "engine");
//    loadcell_message += " ";
//    loadcell_message += get_loadcell_units(nitrous_loadcell, "nitrous");
//    Serial.println(loadcell_message);
//  } else if (engine_loadcell_send) {
//    String loadcell_message = get_loadcell_units(engine_loadcell, "engine");
//    Serial.println(loadcell_message);
////      Serial.println(1234);
//  } else if (nitrous_loadcell_send) {
//    String loadcell_message = get_loadcell_units(nitrous_loadcell, "nitrous");
//    Serial.println(loadcell_message);
////      Serial.println(1234);
//  }
}
