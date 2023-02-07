typedef unsigned char byte;
extern void digitalWrite(uint32_t, uint32_t);
extern void pinMode(uint32_t, uint32_t);

struct Speaker
{
	HardwareTimer *timer;
	int channel;
	int pin;
	PinName pin_name;
	int frequency = 0;
};

#define NUM_SPEAKERS 2
Speaker speakers[NUM_SPEAKERS];

void setup()
{
	// pa 2 tim 5 chan 3 alt1
	speakers[0].pin = PA2;
	speakers[0].pin_name = PA_2_ALT1;
	speakers[0].channel = 3;
	speakers[0].timer = new HardwareTimer(TIM5);

	// pa 6 tim 3 chan 1

	speakers[1].pin = PA6;
	speakers[1].pin_name = PA_6;
	speakers[1].channel = 1;
	speakers[1].timer = new HardwareTimer(TIM3);

	for (int i = 0; i < NUM_SPEAKERS; i++)
	{
		speakers[i].timer->setPWM(speakers[i].channel, speakers[i].pin_name, 200 + 100 * i, 50);
		delay(250);
		speakers[i].timer->pause();
	}

	pinMode(LED_BUILTIN, OUTPUT);
	Serial.begin(250000);
	Serial.setTimeout(1); // high timeout is not necessary with such a high baudrate
}

#define SERIAL_BUFFER_LEN 64
byte buf[SERIAL_BUFFER_LEN];

void loop()
{

	while (!Serial.available())
	{
	}
	memset(buf, 0, SERIAL_BUFFER_LEN);
	digitalWrite(LED_BUILTIN, LOW);
	/*
	Serial.readBytes((char*)buf, SERIAL_BUFFER_LEN);*/

	// might be better to manually read bytes here instead to avoid timeout

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
		uint16_t frequency = ((uint16_t)buf[1] << 8) | ((uint16_t)buf[2]);
		uint8_t velocity = buf[3];

		if (velocity == 0)
		{
			// turn off a speaker
			for (int i = 0; i < NUM_SPEAKERS; i++)
			{
				if (speakers[i].frequency == frequency)
				{
					speakers[i].frequency = 0;
					speakers[i].timer->pause();
					break;
				}
			}
		}
		else
		{
			// turn on a speaker
			for (int i = 0; i < NUM_SPEAKERS; i++)
			{
				if (speakers[i].frequency == 0)
				{
					speakers[i].frequency = frequency;
					speakers[i].timer->setPWM(speakers[i].channel, speakers[i].pin_name, frequency, 50);
					break;
				}
			}
			/*	speaker should be active now,
				but if there are more notes active than speakers
				some notes will not be played
			*/
		}

		break;
	}
	default:
		break;
	}
	digitalWrite(LED_BUILTIN, HIGH);
}
