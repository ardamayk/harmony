use earthly_audio_core::output_device::{
    AlsaOutputDeviceAdapter, OutputDevice, OutputDeviceCatalog, OutputDeviceError,
};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct FakeAlsaOutputDeviceAdapter {
    devices: Arc<Mutex<Vec<OutputDevice>>>,
}

impl FakeAlsaOutputDeviceAdapter {
    fn new(devices: Vec<OutputDevice>) -> Self {
        Self {
            devices: Arc::new(Mutex::new(devices)),
        }
    }

    fn replace(&self, devices: Vec<OutputDevice>) {
        *self.devices.lock().expect("fake ALSA devices") = devices;
    }
}

impl AlsaOutputDeviceAdapter for FakeAlsaOutputDeviceAdapter {
    fn list_raw_devices(&self) -> Result<Vec<OutputDevice>, String> {
        Ok(self.devices.lock().expect("fake ALSA devices").clone())
    }
}

fn usb_dac() -> OutputDevice {
    OutputDevice {
        id: "hw:2,0".to_owned(),
        name: "USB DAC".to_owned(),
    }
}

#[test]
fn user_can_select_an_enumerated_raw_alsa_output_device() {
    let adapter = FakeAlsaOutputDeviceAdapter::new(vec![usb_dac()]);
    let mut catalog = OutputDeviceCatalog::new(Box::new(adapter));

    assert_eq!(
        catalog.refresh().expect("enumerate devices"),
        vec![usb_dac()]
    );
    assert_eq!(catalog.select("hw:2,0").expect("select USB DAC"), usb_dac());
    assert_eq!(catalog.selected(), Some(&usb_dac()));
}

#[test]
fn selection_rejects_conversion_plugins_and_unknown_hardware() {
    let adapter = FakeAlsaOutputDeviceAdapter::new(vec![usb_dac()]);
    let mut catalog = OutputDeviceCatalog::new(Box::new(adapter));

    assert_eq!(
        catalog.select("default"),
        Err(OutputDeviceError::NotRawHardware)
    );
    assert_eq!(
        catalog.select("hw:9,0"),
        Err(OutputDeviceError::Unavailable)
    );
}

#[test]
fn selected_dac_disconnect_is_observable_after_hotplug_refresh() {
    let adapter = FakeAlsaOutputDeviceAdapter::new(vec![usb_dac()]);
    let mut catalog = OutputDeviceCatalog::new(Box::new(adapter.clone()));
    catalog.select("hw:2,0").expect("select USB DAC");

    adapter.replace(Vec::new());

    assert_eq!(
        catalog.refresh(),
        Err(OutputDeviceError::SelectedDeviceDisconnected(usb_dac()))
    );
    assert_eq!(catalog.selected(), Some(&usb_dac()));
}
