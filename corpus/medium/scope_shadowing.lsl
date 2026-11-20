integer gBase = 3;

integer calculate(integer value)
{
    integer total = gBase;
    {
        integer total = value * 2;
        integer i;
        for (i = 0; i < total; i++)
        {
            if (i % 2 == 0) gBase += 1;
            else if (i % 3 == 0) gBase -= 1;
            else gBase = gBase + 1;
        }
    }

    integer i = 0;
    while (i < value)
    {
        total += i;
        i++;
    }

    do
    {
        total -= 1;
    } while (total > value && total > 0);

    return total;
}

default
{
    state_entry()
    {
        integer answer = calculate(4);
        if (answer > 0) jump finished;
        answer = 0;
        @finished;
        llOwnerSay((string)answer);
    }
}
