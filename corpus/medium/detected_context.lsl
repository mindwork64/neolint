read_detected()
{
    llDetectedKey(0);
}

default
{
    state_entry()
    {
        llDetectedName(0);
        read_detected();
    }

    touch_start(integer detected_total)
    {
        if (detected_total > 0)
        {
            read_detected();
        }
    }
}
