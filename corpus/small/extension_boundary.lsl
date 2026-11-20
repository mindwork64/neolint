default
{
    state_entry()
    {
        llUnknownExtensionCall(gUnknownValue);
    }

    custom_extension_event(integer supplied)
    {
        llOwnerSay(supplied);
        llOwnerSay(extension_value);
    }
}
