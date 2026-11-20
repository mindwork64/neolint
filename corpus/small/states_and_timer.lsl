integer gClicks = 0;
float gInterval = 1.0;

default
{
    state_entry()
    {
        llSetTimerEvent(gInterval);
    }

    touch_start(integer total)
    {
        gClicks += total;
        llOwnerSay((string)gClicks);
    }

    timer()
    {
        llSetText("clicks: " + (string)gClicks, <1.0, 1.0, 1.0>, 1.0);
        if (gClicks > 5) state active;
    }
}

state active
{
    state_entry()
    {
        llSetTimerEvent(0.5);
        llOwnerSay("active");
    }

    timer()
    {
        state default;
    }
}
