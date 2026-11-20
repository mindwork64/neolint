vector gTarget = <0.0, 0.0, 0.0>;
rotation gFacing = <0.0, 0.0, 0.0, 1.0>;
float gSpeed = 0.25;

vector offsetFor(integer step)
{
    float amount = (float)step * 0.1;
    return <amount, 0.0, amount>;
}

moveStep(integer step)
{
    vector offset = offsetFor(step);
    gTarget = gTarget + offset;
    vector next = llGetPos() + offset;
    if (gSpeed > 0.0)
    {
        llSetPos(next);
    }
    llSetRot(gFacing);
}

default
{
    state_entry()
    {
        llSetTimerEvent(gSpeed);
    }

    touch_start(integer total)
    {
        gFacing = llGetRot();
        state moving;
    }
}

state moving
{
    state_entry()
    {
        llSetTimerEvent(gSpeed);
    }

    timer()
    {
        if (llVecDist(llGetPos(), gTarget) < 0.1)
        {
            state idle;
        }
        moveStep(1);
    }

    touch_start(integer total)
    {
        state idle;
    }
}

state idle
{
    state_entry()
    {
        llSetTimerEvent(0.0);
    }

    touch_start(integer total)
    {
        gTarget = llGetPos();
        state moving;
    }
}
