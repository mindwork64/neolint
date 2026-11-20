integer gCounter;

default
{
    state_entry()
    {
        llUnknownExtensionCall(gCounter, extension_value);
    }

    custom_extension_event(integer supplied)
    {
        gCounter += supplied;
        llOwnerSay(extension_value);
    }

    touch_start(integer total)
    {
        gCounter += total;
        llOwnerSay((string)gCounter);
    }

    timer()
    {
        llUnknownTimerExtension();
    }
}
