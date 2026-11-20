integer gMask = 0xFF00FF;
integer gFlags = 0x1;

integer fold(integer value)
{
    return (value & 0xFF) | (value << 1);
}

default
{
    state_entry()
    {
        integer high = 0xFFFFFFFF;
        integer minimum = 0x80000000;
        integer shifted = (gMask >> 8) ^ gFlags;
        gFlags = fold(shifted) + high + minimum;
        while (gFlags & 0x10) gFlags = gFlags >> 1;
        llOwnerSay((string)gFlags);
    }
}
