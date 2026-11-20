integer gCount = 0;

default
{
    state_entry()
    {
        llOwnerSay("Hello, Second Life!");
    }

    touch_start(integer total_number)
    {
        gCount += total_number;
        llOwnerSay((string)gCount);
    }
}

state sleeping
{
    state_entry()
    {
        llSetTimerEvent(0.0);
    }
}
