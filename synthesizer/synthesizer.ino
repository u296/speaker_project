typedef unsigned char byte;
extern void digitalWrite(uint32_t, uint32_t);
extern void pinMode(uint32_t, uint32_t);

/*	PROTOCOL
****************
the protocol used to communicate with the computer
only has a single message, to turn on or off a speaker
with a certain frequency. that message is laid out as
follows:

0x01 FF FF VV

multibyte values are sent in big endian format

the message begins with the byte 0x01
	* F is a 16 bit unsigned integer representing the frequency
	* V is an 8 bit unsigned integer containing the velocity, of
the frequency. If 0 then off, anything else then on
 */

/* SPEAKER
****************
A speaker represents a piezo or other sound-generating
device connected to a certain pin. From which pin it
is connected to one can derive the available hardware
timers to be used and the channel, however this must be
done manually by looking up the values in the datasheet
of the STM32F411CEU6
*/
struct Speaker
{
	HardwareTimer *timer;
	int channel;
	PinName pin_name;
	int frequency = 0;
};

#define NUM_SPEAKERS 2
Speaker speakers[NUM_SPEAKERS];

void setup()
{
	// set up the speakers

	// PA2 TIM5 chan 3 (alt1)
	speakers[0].pin_name = PA_2_ALT1;
	speakers[0].channel = 3;
	speakers[0].timer = new HardwareTimer(TIM5);

	// PA6 TIM3 chan 1

	speakers[1].pin_name = PA_6;
	speakers[1].channel = 1;
	speakers[1].timer = new HardwareTimer(TIM3);

	// introduce all the speakers by playing a separate note  on each
	for (int i = 0; i < NUM_SPEAKERS; i++)
	{
		speakers[i].timer->setPWM(speakers[i].channel, speakers[i].pin_name, 200 + 100 * i, 50);
		delay(250);
		speakers[i].timer->pause();
	}

	pinMode(LED_BUILTIN, OUTPUT); // initialize the builtin LED
	Serial.begin(250000);		  // initialize serial communication at 250 kBaud
}

// buffer used for receiving messages over serial
#define SERIAL_BUFFER_LEN 64
byte buf[SERIAL_BUFFER_LEN];

void loop()
{
	// wait until there is a message available
	while (!Serial.available())
	{
	}

	// turn on the LED, it is active LOW
	digitalWrite(LED_BUILTIN, LOW);

	// clear the buffer
	memset(buf, 0, SERIAL_BUFFER_LEN);

	// read all the bytes, or fill the buffer
	for (int i = 0; i < SERIAL_BUFFER_LEN && Serial.available(); i++)
	{
		int incoming = Serial.read();
		if (incoming != -1)
		{
			buf[i] = (byte)incoming;
		}
		else
		{
			// an error occurred
		}
	}

	switch (buf[0])
	{
	case 0x01:
	{
		// reconstruct the message values
		uint16_t frequency = ((uint16_t)buf[1] << 8) | ((uint16_t)buf[2]);
		uint8_t velocity = buf[3];

		if (velocity == 0)
		{
			// turn off a speaker
			for (int i = 0; i < NUM_SPEAKERS; i++)
			{
				if (speakers[i].frequency == frequency) // find the speaker generating the frequency
				{
					speakers[i].frequency = 0;	// indicate that the speaker is now free
					speakers[i].timer->pause(); // pause the timer to stop generating the sound
					break;
				}
			}
		}
		else
		{
			// turn on a speaker
			for (int i = 0; i < NUM_SPEAKERS; i++)
			{
				if (speakers[i].frequency == 0) // find a free speaker
				{
					speakers[i].frequency = frequency;													 // indicate that it is now occupied
					speakers[i].timer->setPWM(speakers[i].channel, speakers[i].pin_name, frequency, 50); // start playing sound
					break;
				}
			}
			/*	A speaker has now been assigned the
				frequency that the message requested
				if there was one available. If not
				then that note will not be played.
			*/
		}

		break;
	}
	default:
		break;
	}
	digitalWrite(LED_BUILTIN, HIGH); // turn off the LED again
}
