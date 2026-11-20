integer gChannel = -9001;
integer gHandle;
key gOwner;
list gButtons = ["Start", "Stop", "Status"];

showMenu(key target)
{
    llDialog(target, "Choose an action", gButtons, gChannel);
}

handleChoice(string choice)
{
    if (choice == "Start")
    {
        llSetTimerEvent(1.0);
        llOwnerSay("started");
    }
    else if (choice == "Stop")
    {
        llSetTimerEvent(0.0);
        llOwnerSay("stopped");
    }
    else if (choice == "Status")
    {
        llOwnerSay(llGetTimestamp());
    }
}

default
{
    state_entry()
    {
        gOwner = llGetOwner();
        gHandle = llListen(gChannel, "", gOwner, "");
    }

    touch_start(integer total)
    {
        showMenu(llDetectedKey(0));
    }

    listen(integer channel, string name, key id, string message)
    {
        handleChoice(message);
    }

    timer()
    {
        llSetObjectDesc("menu active");
    }
}
