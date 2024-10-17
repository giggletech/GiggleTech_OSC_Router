Giggletech Haptics Server - README
===================================

Overview
--------
This guide will help you set up and configure your Giggletech device, software, and server for use in VRChat and other compatible platforms. Follow the steps below to get started.

Step 1: Configure Your Giggletech Device
----------------------------------------

About Giggletech Devices

Status LED:
- Initial Power-Up: 3 quick blinks.
- Normal Operation: 3 more blinks, 3 buzzes. Light will be off. LED lights up when pat commands are received.
- Configuration Mode: Solid LED.


Enter Configuration Mode:
1. Power up the device (3 blinks).
2. Unplug and re-plug it in. The LED will go solid, indicating configuration mode.

Connect the Device to Wi-Fi:
1. Connect to the "Giggletech_haptics" Wi-Fi network (password: giggletech).
2. Your phone may automatically direct you to the configuration page. If not, open a browser and go to http://192.168.4.1.
3. Select your Wi-Fi network from the list and enter your Wi-Fi password.


IP Setup:
- Automatic (Recommended): Uncheck 'Use static IP' and click 'Save'.
- Static IP (Advanced): Set your IP, Gateway, and Subnet manually.

After saving, unplug and re-plug the device to apply settings. The device will blink 3 times and buzz 3 times, indicating successful connection.

Step 2: Giggletech Software Setup
---------------------------------
Software Package Includes:
1. Giggletech_server_x.x.exe: Communicates with VRChat to send headpats to your Giggletech device.
2. Giggletech_VRC_Simulator.exe: Tests system configuration by emulating VRChat.
3. Giggletech_OSCQuery_Installer.exe: Ensures compatibility with other OSC-based programs.

Install the Software:
1. Download the latest version from the Giggletech GitHub.
2. Run the Giggletech_OSCQuery_Installer.exe to install the OSCQuery Helper.

Step 3: Configure the YML File
------------------------------
1. Find Your Device's IP:
   Visit http://giggletech.local. If it doesn’t load, ensure the Bonjour Service is installed from https://developer.apple.com/bonjour/.

2. Edit the YML File:
   Open "config.yml" using Notepad. Replace the default IP (192.168.1.69) with the one obtained from giggletech.local or your static IP setup.

3. Multiple Devices:
   Add multiple devices to the YML by copying the format and assigning unique IPs. Power up each device separately.

4. Save Your Changes:
   After updating, save the file and close the editor.

Step 4: Test Your System
------------------------
Before entering VRChat:
1. Open the Giggletech Server software.
2. Run gigglepuck_vrc_simulator.exe to emulate a VRChat environment.
3. Follow the simulator prompts to confirm your hardware setup.

**Note**: The Giggletech Server must remain open while using VRChat to receive haptic interactions.

For further assistance, visit our Discord or contact us via email.
