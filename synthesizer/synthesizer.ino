// to get VScode to stop complaining
typedef unsigned char byte;
extern void digitalWrite(uint32_t, uint32_t);
extern void pinMode(uint32_t, uint32_t);

/*	PROTOCOL
****************
multibyte values are sent in big endian format

update message: turn on or off a speaker
with a certain frequency.
layout:

0x01 FF FF VV 0x01

the message begins with the byte 0x01
	* F is a 16 bit unsigned integer representing the frequency
	* V is an 8 bit unsigned integer containing the velocity, of
the frequency. If 0 then off, anything else then on
the message ends with the byte 0x01

reset message: turn off all speakers
layout:

0x02
 */

enum class MessageType
{
	NoteUpdate = 0x01,
	Reset = 0x02
};

enum class MessageLength
{
	NoteUpdate = 5,
	Reset = 1
};

/* SPEAKER
****************
A speaker represents a piezo or other sound-generating
device connected to a certain pin. From which pin it
is connected to one can derive the available hardware
timers to be used and the channel, however this must be
done manually by looking up the values in the datasheet
of the STM32F411CEU6

frequency = 0 indicates that the speaker is not in use
*/
struct Speaker
{
	HardwareTimer *timer;
	int channel;
	PinName pin_name;
	int frequency = 0;

	void play_frequency(int freq)
	{
		this->frequency = freq;
		this->timer->setPWM(this->channel, this->pin_name, this->frequency, 50);
	}

	void turn_off()
	{
		this->frequency = 0;
		this->timer->pause();
	}
};

#define NUM_SPEAKERS 6
Speaker speakers[NUM_SPEAKERS];

#define BAUDRATE 250000

// buffer used for receiving messages over serial
#define SERIAL_BUFFER_LEN 64
byte serial_buf[SERIAL_BUFFER_LEN];
uint8_t cursor_pos = 0;

void setup_speakers()
{
	// PA 8 TIM 1 chan 1
	speakers[0].pin_name = PA_8;
	speakers[0].channel = 1;
	speakers[0].timer = new HardwareTimer(TIM1);

	// PB 6 TIM 4 chan 1

	speakers[1].pin_name = PB_6;
	speakers[1].channel = 1;
	speakers[1].timer = new HardwareTimer(TIM4);

	// PA 3 TIM 9 chan 2 alt 2

	speakers[2].pin_name = PA_3_ALT2;
	speakers[2].channel = 2;
	speakers[2].timer = new HardwareTimer(TIM9);

	// PA 1 TIM2 chan 2

	speakers[3].pin_name = PA_1;
	speakers[3].channel = 2;
	speakers[3].timer = new HardwareTimer(TIM2);

	// PA2 TIM5 chan 3 (alt1)
	speakers[4].pin_name = PA_2_ALT1;
	speakers[4].channel = 3;
	speakers[4].timer = new HardwareTimer(TIM5);

	// PA6 TIM3 chan 1

	speakers[5].pin_name = PA_6;
	speakers[5].channel = 1;
	speakers[5].timer = new HardwareTimer(TIM3);
}

void test_speakers()
{
	for (int i = 0; i < NUM_SPEAKERS; i++)
	{
		speakers[i].play_frequency(200 * pow(1.5, i));
		delay(250);
		speakers[i].turn_off();
	}
}

void setup()
{
	setup_speakers();
	test_speakers();

	Serial.begin(BAUDRATE);
	memset(serial_buf, 0, SERIAL_BUFFER_LEN);
	pinMode(LED_BUILTIN, OUTPUT);
}

void wait_for_message()
{
	digitalWrite(LED_BUILTIN, HIGH);
	while (!Serial.available())
	{
	}
	digitalWrite(LED_BUILTIN, LOW);
}

void read_from_serial()
{
	for (; cursor_pos < SERIAL_BUFFER_LEN && Serial.available(); cursor_pos++)
	{
		int incoming = Serial.read();
		if (incoming != -1)
		{
			serial_buf[cursor_pos] = (byte)incoming;
		}
		else
		{
			// an error occurred
		}
	}
}

void update_note(uint16_t frequency, uint8_t velocity)
{
	if (velocity == 0)
	{
		// turn off a speaker
		for (int i = 0; i < NUM_SPEAKERS; i++)
		{
			if (speakers[i].frequency == frequency) // find the speaker generating the frequency
			{
				speakers[i].turn_off();
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
				speakers[i].play_frequency(frequency);
				break;
			}
		}
		/*	A speaker has now been assigned the
			frequency that the message requested
			if there was one available. If not
			then that note will not be played.
		*/
	}
}

void pop_message(uint8_t message_length)
{
	/* move everything in the serial buffer to the left by message_length

		A B C D E F G H I J K L M N O P
					  ^
		\_____/       |
	  message_len   cursor

		to

		E F G H I J K L M N O P
			  ^
			  |
			cursor

		this removes the first message_length bytes from the array, and
		the cursor will still point to the same value
	*/
	memmove(serial_buf, serial_buf + message_length, SERIAL_BUFFER_LEN - message_length);
	cursor_pos -= message_length;
}

void loop()
{
	read_from_serial();

	if (cursor_pos == 0)
	{
		/*
		after maybe reading, we are still about to write
		to the first byte of the buffer. This means that
		no bytes were written.
		*/
		wait_for_message();
		return;
	}

	switch (serial_buf[0])
	{
	case static_cast<uint8_t>(MessageType::NoteUpdate):
	{
		// 0x01 FF FF VV 0x01

		// check that we have a complete message
		if (cursor_pos < static_cast<uint8_t>(MessageLength::NoteUpdate))
		{
			wait_for_message();
			break;
		}

		// reconstruct the message values
		uint16_t frequency = ((uint16_t)serial_buf[1] << 8) | ((uint16_t)serial_buf[2]);
		uint8_t velocity = serial_buf[3];

		update_note(frequency, velocity);

		pop_message(static_cast<uint8_t>(MessageLength::NoteUpdate));

		break;
	}
	case static_cast<uint8_t>(MessageType::Reset):
	{
		// 0x02

		if (cursor_pos < static_cast<uint8_t>(MessageLength::Reset))
		{
			wait_for_message();
			break;
		}

		for (int i = 0; i < NUM_SPEAKERS; i++)
		{
			speakers[i].turn_off();
		}

		pop_message(static_cast<uint8_t>(MessageLength::Reset));
	}
	default:
		break;
	}
}
