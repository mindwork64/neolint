key gOwner;
integer gChannel = -12001;
integer gListener;
integer gBusy;
list gDestinations = ["Home", "Market", "Workshop"];
vector gHome = <128.0, 128.0, 22.0>;
rotation gHomeRotation = <0.0, 0.0, 0.0, 1.0>;

showMenu(key target)
{
    llDialog(target, "Where do you want to go?", gDestinations, gChannel);
}

integer findDestination(string name)
{
    integer count = llGetListLength(gDestinations);
    integer i;
    for (i = 0; i < count; i++)
    {
        if (llList2String(gDestinations, i) == name) return i;
    }
    return -1;
}

vector destinationPosition(string name)
{
    if (name == "Market") return <96.0, 160.0, 24.0>;
    if (name == "Workshop") return <64.0, 96.0, 21.0>;
    return gHome;
}

moveTo(vector position)
{
    llSetPos(position);
    llSetRot(gHomeRotation);
}

default
{
    state_entry()
    {
        gOwner = llGetOwner();
        gListener = llListen(gChannel, "", gOwner, "");
        llSetText("Teleporter ready", <1.0, 1.0, 1.0>, 1.0);
    }

    touch_start(integer total)
    {
        showMenu(llDetectedKey(0));
    }

    listen(integer channel, string name, key id, string message)
    {
        integer index = findDestination(message);
        if (index >= 0)
        {
            state traveling;
        }
        else
        {
            llInstantMessage(gOwner, "Unknown destination: " + message);
        }
    }
}

state traveling
{
    state_entry()
    {
        gBusy = TRUE;
        llSetTimerEvent(0.5);
        llSetText("Traveling", <1.0, 0.5, 0.0>, 1.0);
    }

    timer()
    {
        llSetTimerEvent(0.0);
        gBusy = FALSE;
        moveTo(destinationPosition("Home"));
        llSetText("Teleporter ready", <1.0, 1.0, 1.0>, 1.0);
        state default;
    }

    touch_start(integer total)
    {
        llInstantMessage(gOwner, "Please wait");
    }
}
