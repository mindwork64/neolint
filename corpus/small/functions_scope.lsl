integer gBase = 2;

integer scale(integer value, integer factor)
{
    return value * factor;
}

integer apply(integer value)
{
    integer result = scale(value, gBase);
    {
        integer result = value + 1;
        gBase = result;
    }
    return result;
}

default
{
    state_entry()
    {
        llOwnerSay((string)apply(3));
    }
}
